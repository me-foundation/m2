use mpl_token_metadata::state::{Metadata, TokenMetadataAccount};
use solana_program::program::invoke;

use {
    crate::constants::*,
    crate::errors::ErrorCode,
    crate::states::*,
    crate::utils::*,
    anchor_lang::{
        prelude::*,
        solana_program::{program::invoke_signed, system_instruction},
        AnchorDeserialize,
    },
    anchor_spl::{
        associated_token::AssociatedToken,
        token::{SetAuthority, Token, TokenAccount},
    },
    solana_program::program_option::COption,
    spl_token::instruction::AuthorityType,
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
    #[account(mut,
      close=seller,
      constraint= seller_trade_state.buyer_price == buyer_price,
      constraint= seller_trade_state.token_size == token_size,
      constraint= seller_trade_state.expiry == seller_state_expiry,
      constraint= seller_trade_state.seller_referral == seller_referral.key(),
      seeds=[
        PREFIX.as_bytes(),
        seller.key().as_ref(),
        auction_house.key().as_ref(),
        token_account.key().as_ref(),
        token_mint.key().as_ref(),
      ], bump=seller_trade_state.bump)]
    seller_trade_state: Box<Account<'info, SellerTradeState>>,
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
    let buyer = &ctx.accounts.buyer.to_account_info();
    let seller = &ctx.accounts.seller.to_account_info();
    let authority = &ctx.accounts.authority.to_account_info();
    let notary = &ctx.accounts.notary;
    let token_account = &ctx.accounts.token_account;
    let token_mint = &ctx.accounts.token_mint;
    let metadata = &ctx.accounts.metadata;
    let buyer_receipt_token_account = &ctx.accounts.buyer_receipt_token_account;
    let escrow_payment_account = &ctx.accounts.escrow_payment_account;
    let auction_house = &ctx.accounts.auction_house;
    let auction_house_treasury = &ctx.accounts.auction_house_treasury;
    let buyer_trade_state = &mut ctx.accounts.buyer_trade_state;
    let seller_trade_state = &mut ctx.accounts.seller_trade_state;
    let token_program = &ctx.accounts.token_program;
    let system_program = &ctx.accounts.system_program;
    let ata_program = &ctx.accounts.ata_program;
    let program_as_signer = &ctx.accounts.program_as_signer;
    let rent = &ctx.accounts.rent;

    let token_clone = token_program.to_account_info();
    let treasury_clone = auction_house_treasury.to_account_info();
    let buyer_receipt_clone = buyer_receipt_token_account.to_account_info();
    let token_account_clone = token_account.to_account_info();

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
        return Err(ErrorCode::NoValidSignerPresent.into());
    }

    if buyer_trade_state.data_is_empty() || seller_trade_state.to_account_info().data_is_empty() {
        return Err(ErrorCode::BothPartiesNeedToAgreeToSale.into());
    }
    let bid_args = BidArgs::from_account_info(buyer_trade_state)?;
    bid_args.check_args(
        ctx.accounts.buyer_referral.key,
        buyer_price,
        token_mint.key,
        token_size,
    )?;

    let clock = Clock::get()?;
    if bid_args.expiry.abs() > 1 && clock.unix_timestamp > bid_args.expiry.abs() {
        return Err(ErrorCode::InvalidExpiry.into());
    }
    if seller_trade_state.expiry.abs() > 1 && clock.unix_timestamp > seller_trade_state.expiry.abs()
    {
        return Err(ErrorCode::InvalidExpiry.into());
    }

    let mut payer = authority;
    if buyer.is_signer {
        payer = buyer
    } else if seller.is_signer {
        payer = seller
    }

    let delegate = get_delegate_from_token_account(&token_account_clone)?;
    if let Some(d) = delegate {
        assert_keys_equal(program_as_signer.key(), d)?;
    } else if !is_token_owner(&token_account_clone, &program_as_signer.key())? {
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
    let wallet_key = buyer.key();
    let escrow_signer_seeds = [
        PREFIX.as_bytes(),
        auction_house_key.as_ref(),
        wallet_key.as_ref(),
        &[escrow_payment_bump],
    ];

    let royalty = if bid_args.buyer_creator_royalty_bp == 0 {
        0
    } else {
        pay_creator_fees(
            &mut ctx.remaining_accounts.iter(),
            None,
            &Metadata::from_account_info(metadata)?,
            &escrow_payment_account.to_account_info(),
            system_program,
            &escrow_signer_seeds,
            buyer_price,
            bid_args.buyer_creator_royalty_bp,
        )?
    };

    // payer pays maker/taker fees
    // seller is payer and taker
    //   seller as payer pays (maker_fee + taker_fee) to treasury
    //   buyer as maker needs to pay args.price + maker_fee + royalty
    //   seller gets (args.price + maker_fee) from buyer
    // buyer is payer and taker
    //   buyer as payer pays (maker_fee + taker_fee) to treasury
    //   buyer as taker needs to pay (args.price + taker_fee + royalty)
    //   seller gets (args.price - maker_fee) from buyer
    // royalty is also paid ON TOP of the price
    let (actual_maker_fee_bp, actual_taker_fee_bp) =
        get_actual_maker_taker_fee_bp(notary, maker_fee_bp, taker_fee_bp);
    let maker_fee = (buyer_price as i128)
        .checked_mul(actual_maker_fee_bp as i128)
        .ok_or(ErrorCode::NumericalOverflow)?
        .checked_div(10000)
        .ok_or(ErrorCode::NumericalOverflow)? as i64;
    let taker_fee = (buyer_price as u128)
        .checked_mul(actual_taker_fee_bp as u128)
        .ok_or(ErrorCode::NumericalOverflow)?
        .checked_div(10000)
        .ok_or(ErrorCode::NumericalOverflow)? as u64;
    let seller_will_get_from_buyer = if payer.key.eq(seller.key) {
        (buyer_price as i64)
            .checked_add(maker_fee)
            .ok_or(ErrorCode::NumericalOverflow)?
    } else {
        (buyer_price as i64)
            .checked_sub(maker_fee)
            .ok_or(ErrorCode::NumericalOverflow)?
    } as u64;
    let total_platform_fee = (maker_fee
        .checked_add(taker_fee as i64)
        .ok_or(ErrorCode::NumericalOverflow)?) as u64;

    invoke_signed(
        &system_instruction::transfer(
            escrow_payment_account.key,
            seller.key,
            seller_will_get_from_buyer,
        ),
        &[
            escrow_payment_account.to_account_info(),
            seller.to_account_info(),
            system_program.to_account_info(),
        ],
        &[&escrow_signer_seeds],
    )?;

    if total_platform_fee > 0 {
        if payer.key == seller.key {
            invoke(
                &system_instruction::transfer(
                    payer.key,
                    auction_house_treasury.key,
                    total_platform_fee,
                ),
                &[
                    payer.to_account_info(),
                    treasury_clone,
                    system_program.to_account_info(),
                ],
            )?;
        } else {
            invoke_signed(
                &system_instruction::transfer(
                    escrow_payment_account.key,
                    auction_house_treasury.key,
                    total_platform_fee,
                ),
                &[
                    escrow_payment_account.to_account_info(),
                    treasury_clone,
                    system_program.to_account_info(),
                ],
                &[&escrow_signer_seeds],
            )?;
        }
    }

    if buyer_receipt_token_account.data_is_empty() {
        make_ata(
            buyer_receipt_token_account.to_account_info(),
            payer.to_account_info(),
            buyer.to_account_info(),
            token_mint.to_account_info(),
            ata_program.to_account_info(),
            token_program.to_account_info(),
            system_program.to_account_info(),
            rent.to_account_info(),
        )?;
    }

    let buyer_rec_acct = assert_is_ata(
        &buyer_receipt_clone,
        &buyer.key(),
        &token_mint.key(),
        &buyer.key(),
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

    let program_as_signer_seeds = [
        PREFIX.as_bytes(),
        SIGNER.as_bytes(),
        &[program_as_signer_bump],
    ];

    invoke_signed(
        &spl_token::instruction::transfer(
            token_program.key,
            &token_account.key(),
            &buyer_receipt_token_account.key(),
            &program_as_signer.key(),
            &[],
            token_size,
        )?,
        &[
            token_account_clone,
            buyer_receipt_clone,
            program_as_signer.to_account_info(),
            token_clone,
        ],
        &[&program_as_signer_seeds],
    )?;

    // if we hold the token_account ownership from program_as_signer,
    // we'd like to return that authority back to the original seller
    if is_token_owner(token_account, &program_as_signer.key())? {
        let (_program_as_signer, program_as_signer_bump) =
            Pubkey::find_program_address(&[PREFIX.as_bytes(), SIGNER.as_bytes()], ctx.program_id);
        let seeds = &[
            PREFIX.as_bytes(),
            SIGNER.as_bytes(),
            &[program_as_signer_bump][..],
        ];
        let mut token_acc = Account::<TokenAccount>::try_from(token_account)?;
        token_acc.reload()?;
        if token_acc.amount == 0 {
            invoke_signed(
                &spl_token::instruction::close_account(
                    token_program.key,
                    &token_account.key(),
                    &seller.key(),
                    &program_as_signer.key(),
                    &[],
                )?,
                &[
                    token_account.to_account_info(),
                    seller.to_account_info(),
                    program_as_signer.to_account_info(),
                    token_program.to_account_info(),
                ],
                &[&seeds[..]],
            )?;
        } else {
            anchor_spl::token::set_authority(
                CpiContext::new(
                    token_program.to_account_info(),
                    SetAuthority {
                        account_or_mint: token_account.to_account_info(),
                        current_authority: program_as_signer.to_account_info(),
                    },
                )
                .with_signer(&[&seeds[..]]),
                AuthorityType::AccountOwner,
                Some(seller.key()),
            )?;
        }
    }

    // zero-out the token_size so that we don't accidentally use it again
    seller_trade_state.token_size = 0;

    // we don't need to zero out buyer_trade_state, just copy zero discriminator to it and then close
    close_account_anchor(buyer_trade_state, buyer)?;

    try_close_buyer_escrow(
        escrow_payment_account,
        buyer,
        system_program,
        &[&escrow_signer_seeds],
    )?;

    msg!(
        "{{\"price\":{},\"seller_expiry\":{},\"buyer_expiry\":{},\"royalty\":{}}}",
        buyer_price,
        seller_trade_state.expiry,
        bid_args.expiry,
        royalty,
    );

    Ok(())
}
