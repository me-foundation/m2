use mpl_token_metadata::accounts::Metadata;

use crate::index_ra;

use {
    crate::constants::*,
    crate::errors::ErrorCode,
    crate::states::*,
    crate::utils::*,
    anchor_lang::{prelude::*, AnchorDeserialize},
    anchor_spl::{associated_token::AssociatedToken, token::Token},
    solana_program::program_option::COption,
};

#[derive(Accounts)]
#[instruction(
    escrow_payment_bump: u8,
    program_as_signer_bump: u8,
    buyer_price: u64,
    token_size: u64,
    buyer_state_expiry: i64,
    seller_state_expiry: i64,
    maker_fee_bp: i16,
    taker_fee_bp: u16
)]
pub struct ExecuteSaleV2<'info> {
    /// CHECK: buyer. Either buyer or the seller has to be the signer
    #[account(mut)]
    buyer: UncheckedAccount<'info>,
    /// CHECK: seller. Either buyer or the seller has to be the signer
    #[account(mut)]
    seller: UncheckedAccount<'info>,
    /// CHECK: optional
    notary: UncheckedAccount<'info>,
    /// CHECK: token_account
    #[account(mut)]
    token_account: UncheckedAccount<'info>,
    /// CHECK: token_mint
    token_mint: UncheckedAccount<'info>,
    /// CHECK: metadata
    #[account(
    seeds = [
        "metadata".as_bytes(),
        mpl_token_metadata::ID.as_ref(),
        token_mint.key().as_ref(),
    ],
    bump,
    seeds::program = mpl_token_metadata::ID,
    )]
    metadata: UncheckedAccount<'info>,
    /// CHECK: escrow_payment_account
    #[account(
        mut,
        seeds=[
            PREFIX.as_bytes(),
            auction_house.key().as_ref(),
            buyer.key().as_ref()
        ],
        bump=escrow_payment_bump,
        constraint= maker_fee_bp <= MAX_MAKER_FEE_BP @ ErrorCode::InvalidPlatformFeeBp,
        constraint= maker_fee_bp >= -(taker_fee_bp as i16) @ ErrorCode::InvalidPlatformFeeBp,
        constraint= taker_fee_bp <= MAX_TAKER_FEE_BP @ ErrorCode::InvalidPlatformFeeBp,
    )]
    escrow_payment_account: UncheckedAccount<'info>,
    /// CHECK: buyer_receipt_token_account
    #[account(mut)]
    buyer_receipt_token_account: UncheckedAccount<'info>,
    /// CHECK: authority
    authority: UncheckedAccount<'info>,
    #[account(
        seeds=[PREFIX.as_bytes(), auction_house.creator.as_ref()],
        bump=auction_house.bump,
        has_one=authority,
        has_one=auction_house_treasury,
        constraint = auction_house.notary == notary.key() @ ErrorCode::InvalidNotary,
    )]
    auction_house: Account<'info, AuctionHouse>,
    /// CHECK: auction_house_treasury
    #[account(mut, seeds=[PREFIX.as_bytes(), auction_house.key().as_ref(), TREASURY.as_bytes()], bump=auction_house.treasury_bump)]
    auction_house_treasury: UncheckedAccount<'info>,
    /// CHECK: check seeds and check bid_args
    #[account(
        mut,
        seeds=[
          PREFIX.as_bytes(),
          buyer.key().as_ref(),
          auction_house.key().as_ref(),
          token_mint.key().as_ref(),
        ],
        bump
    )]
    buyer_trade_state: AccountInfo<'info>,
    /// CHECK: buyer_referral
    #[account(mut)]
    buyer_referral: UncheckedAccount<'info>,
    /// CHECK: check seeds and check sell_args
    #[account(
        mut,
        seeds=[
          PREFIX.as_bytes(),
          seller.key().as_ref(),
          auction_house.key().as_ref(),
          token_account.key().as_ref(),
          token_mint.key().as_ref(),
        ],
        bump
    )]
    seller_trade_state: AccountInfo<'info>,
    /// CHECK: seller_referral
    #[account(mut)]
    seller_referral: UncheckedAccount<'info>,
    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,
    ata_program: Program<'info, AssociatedToken>,
    /// CHECK: program_as_signer
    #[account(seeds=[PREFIX.as_bytes(), SIGNER.as_bytes()], bump)]
    program_as_signer: UncheckedAccount<'info>,
    rent: Sysvar<'info, Rent>,
    // remaining accounts:
    // ** IF USING NATIVE SOL **
    // 0..=4. creators (optional) - if the buyer is paying in SOL, these are the creators of the token
    //
    // ** IF USING SPL **
    // 0. payment_mint (required) - if the buyer is paying in a token, this is the mint of that token
    // 1. payment_source_token_account (required) - escrow token account controlled by escrow_payment_account
    // 2. payment_seller_token_account (required) - token account controlled by seller
    // 3. payment_treausry_token_account (required) - token account controlled by auction_house_treasury
    // 4..=13. creator_token_account (optional) - if the buyer is paying in a SPL token, these are the creator token accounts,
    //                                            if the creator token accounts are not initialized, the creator itself needs to be
    //                                            included, in the format of creator_1_ATA, creator_1, creator_2_ATA, creator_2, ...
    // ...
    // -1. payer (optional) - this wallet will try to pay for rent
}

pub fn handle<'info>(
    ctx: Context<'_, '_, '_, 'info, ExecuteSaleV2<'info>>,
    escrow_payment_bump: u8,
    program_as_signer_bump: u8,
    buyer_price: u64,
    token_size: u64,
    maker_fee_bp: i16,
    taker_fee_bp: u16,
) -> Result<()> {
    let (remaining_accounts, possible_payer) =
        split_payer_from_remaining_accounts(ctx.remaining_accounts);
    let buyer = &ctx.accounts.buyer;
    let seller = &ctx.accounts.seller;
    let notary = &ctx.accounts.notary;
    let token_account = &ctx.accounts.token_account;
    let token_mint = &ctx.accounts.token_mint;
    let metadata = &ctx.accounts.metadata;
    let buyer_receipt_token_account = &ctx.accounts.buyer_receipt_token_account;
    let escrow_payment_account = &ctx.accounts.escrow_payment_account;
    let auction_house = &ctx.accounts.auction_house;
    let auction_house_treasury = &ctx.accounts.auction_house_treasury;
    let buyer_trade_state = &ctx.accounts.buyer_trade_state;
    let seller_trade_state = &ctx.accounts.seller_trade_state;
    let token_program = &ctx.accounts.token_program;
    let system_program = &ctx.accounts.system_program;
    let program_as_signer = &ctx.accounts.program_as_signer;

    assert_bump(
        &[
            PREFIX.as_bytes(),
            auction_house.key().as_ref(),
            buyer.key().as_ref(),
        ],
        ctx.program_id,
        escrow_payment_bump,
    )?;

    if !buyer.is_signer && !seller.is_signer {
        return Err(ErrorCode::SaleRequiresSigner.into());
    }

    if buyer_trade_state.data_is_empty() || seller_trade_state.to_account_info().data_is_empty() {
        return Err(ErrorCode::BothPartiesNeedToAgreeToSale.into());
    }
    let bid_args = BidArgs::from_account_info(buyer_trade_state)?;
    let is_spl = bid_args.payment_mint != Pubkey::default();

    bid_args.check_args(
        ctx.accounts.buyer_referral.key,
        buyer_price,
        token_mint.key,
        token_size,
        if is_spl {
            index_ra!(remaining_accounts, 0).key // mint account
        } else {
            &bid_args.payment_mint
        },
    )?;
    let sell_args = SellArgs::from_account_info(seller_trade_state)?;
    sell_args.check_args(
        ctx.accounts.seller_referral.key,
        &buyer_price,
        token_mint.key,
        &token_size,
        &bid_args.payment_mint, // check that mints match, equality is transitive
    )?;

    let clock = Clock::get()?;
    if bid_args.expiry.abs() > 1 && clock.unix_timestamp > bid_args.expiry.abs() {
        return Err(ErrorCode::InvalidExpiry.into());
    }
    if sell_args.expiry.abs() > 1 && clock.unix_timestamp > sell_args.expiry.abs() {
        return Err(ErrorCode::InvalidExpiry.into());
    }

    let taker = if buyer.is_signer { buyer } else { seller };
    let payer = if let Some(p) = possible_payer {
        p
    } else {
        taker
    };

    let delegate = get_delegate_from_token_account(token_account)?;
    if let Some(d) = delegate {
        assert_keys_equal(program_as_signer.key, &d)?;
    } else if !is_token_owner(token_account, &program_as_signer.key())? {
        return Err(ErrorCode::IncorrectOwner.into());
    }

    assert_is_ata(
        &token_account.to_account_info(),
        &seller.key(),
        token_mint.key,
        &program_as_signer.key(),
    )?;

    assert_metadata_valid(metadata, token_mint.key)?;

    let auction_house_key = auction_house.key();
    let escrow_signer_seeds: &[&[&[u8]]] = &[&[
        PREFIX.as_bytes(),
        auction_house_key.as_ref(),
        buyer.key.as_ref(),
        &[escrow_payment_bump],
    ]];

    let royalty = if bid_args.buyer_creator_royalty_bp == 0 {
        0
    } else {
        pay_creator_fees(
            &mut (if is_spl {
                remaining_accounts[4..].iter()
            } else {
                remaining_accounts.iter()
            }),
            None,
            &Metadata::safe_deserialize(&metadata.data.borrow())?,
            &escrow_payment_account.to_account_info(),
            escrow_signer_seeds,
            buyer_price,
            bid_args.buyer_creator_royalty_bp,
            if is_spl {
                Some(TransferCreatorSplArgs {
                    buyer,
                    payer,
                    mint: index_ra!(remaining_accounts, 0),
                    payment_source_token_account: index_ra!(remaining_accounts, 1),
                    system_program,
                    token_program,
                })
            } else {
                None
            },
        )?
    };

    let (actual_maker_fee_bp, actual_taker_fee_bp) =
        get_actual_maker_taker_fee_bp(notary, maker_fee_bp, taker_fee_bp);
    transfer_listing_payment(
        buyer_price,
        actual_maker_fee_bp,
        actual_taker_fee_bp,
        taker,
        seller,
        escrow_payment_account,
        auction_house_treasury,
        if is_spl {
            Some(TransferListingPaymentSplArgs {
                payer,
                buyer,
                mint: index_ra!(remaining_accounts, 0),
                payment_source_token_account: index_ra!(remaining_accounts, 1),
                payment_seller_token_account: index_ra!(remaining_accounts, 2),
                payment_treasury_token_account: index_ra!(remaining_accounts, 3),
                system_program,
                token_program,
            })
        } else {
            None
        },
        escrow_signer_seeds,
    )?;

    let buyer_rec_acct = transfer_token(
        &token_size,
        payer,
        program_as_signer,
        seller,
        None,
        DestinationSpecifier::Ai(buyer),
        token_mint,
        token_account,
        buyer_receipt_token_account,
        token_program,
        system_program,
        None,
        &[&[
            PREFIX.as_bytes(),
            SIGNER.as_bytes(),
            &[program_as_signer_bump],
        ]],
    )?;
    // If the buyer receipt token account's delegate is not nil and is not the same as
    // program_as_signer, then we think it might be safe to not do the transfer to prevent rug
    match buyer_rec_acct.delegate {
        COption::Some(delegate) if program_as_signer.key() != delegate => {
            return Err(ErrorCode::BuyerATACannotHaveDelegate.into());
        }
        _ => {
            // do nothing
        }
    }

    // we don't need to zero out buyer_trade_state, just copy zero discriminator to it and then close
    close_account_anchor(buyer_trade_state, buyer)?;
    close_account_anchor(seller_trade_state, seller)?;

    try_close_buyer_escrow(
        escrow_payment_account,
        buyer,
        system_program,
        escrow_signer_seeds,
    )?;

    msg!(
        "{{\"price\":{},\"seller_expiry\":{},\"buyer_expiry\":{},\"royalty\":{}}}",
        buyer_price,
        sell_args.expiry,
        bid_args.expiry,
        royalty,
    );

    Ok(())
}
