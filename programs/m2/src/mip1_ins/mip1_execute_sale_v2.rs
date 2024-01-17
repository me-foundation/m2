use std::collections::HashMap;

use mpl_token_metadata::{
    accounts::Metadata,
    instructions::TransferBuilder,
    types::{AuthorizationData, Payload, PayloadType, SeedsVec, TransferArgs},
};
use solana_program::{program::invoke_signed, sysvar};

use crate::index_ra;

use {
    crate::constants::*,
    crate::errors::ErrorCode,
    crate::states::*,
    crate::utils::*,
    anchor_lang::prelude::*,
    anchor_spl::{
        associated_token::AssociatedToken,
        token::{Mint, Token, TokenAccount},
    },
};

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct MIP1ExecuteSaleV2Args {
    pub price: u64,
    pub maker_fee_bp: i16,
    pub taker_fee_bp: u16,
}

#[derive(Accounts)]
#[instruction(args:MIP1ExecuteSaleV2Args)]
pub struct MIP1ExecuteSaleV2<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: buyer
    #[account(mut)]
    pub buyer: UncheckedAccount<'info>,
    /// CHECK: seller
    #[account(mut)]
    pub seller: UncheckedAccount<'info>,
    /// CHECK: optional
    pub notary: UncheckedAccount<'info>,
    /// CHECK: program_as_signer
    #[account(seeds=[PREFIX.as_bytes(), SIGNER.as_bytes()], bump)]
    pub program_as_signer: UncheckedAccount<'info>,
    #[account(
        mut,
        token::mint = token_mint,
        constraint = token_account.amount == 1,
        constraint = token_account.owner == seller.key() || token_account.owner == program_as_signer.key() @ ErrorCode::IncorrectOwner
    )]
    pub token_account: Box<Account<'info, TokenAccount>>,
    /// CHECK: checked in cpi
    #[account(mut)]
    pub buyer_receipt_token_account: UncheckedAccount<'info>,
    #[account(
        constraint = token_mint.supply == 1 && token_mint.decimals == 0,
    )]
    pub token_mint: Box<Account<'info, Mint>>,
    /// CHECK: metadata
    #[account(
    mut,
    seeds = [
        "metadata".as_bytes(),
        mpl_token_metadata::ID.as_ref(),
        token_mint.key().as_ref(),
    ],
    bump,
    seeds::program = mpl_token_metadata::ID,
    )]
    pub metadata: UncheckedAccount<'info>,
    #[account(
        seeds=[PREFIX.as_bytes(), auction_house.creator.as_ref()],
        constraint = auction_house.notary == notary.key() @ ErrorCode::InvalidNotary,
        bump,
    )]
    pub auction_house: Box<Account<'info, AuctionHouse>>,
    /// CHECK: auction_house_treasury
    #[account(mut, seeds=[PREFIX.as_bytes(), auction_house.key().as_ref(), TREASURY.as_bytes()], bump)]
    pub auction_house_treasury: UncheckedAccount<'info>,
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
        bump,
    )]
    pub seller_trade_state: AccountInfo<'info>,
    /// CHECK: check seeds and check bid_args
    #[account(
        mut,
        seeds=[
            PREFIX.as_bytes(),
            buyer.key().as_ref(),
            auction_house.key().as_ref(),
            token_mint.key().as_ref(),
        ],
        bump,
    )]
    pub buyer_trade_state: AccountInfo<'info>,
    /// CHECK: check with contraints
    #[account(
        mut,
        seeds=[PREFIX.as_bytes(), auction_house.key().as_ref(), buyer.key().as_ref()],
        constraint= args.price > 0,
        constraint= args.maker_fee_bp <= MAX_MAKER_FEE_BP @ ErrorCode::InvalidPlatformFeeBp,
        constraint= args.maker_fee_bp >= -(args.taker_fee_bp as i16) @ ErrorCode::InvalidPlatformFeeBp,
        constraint= args.taker_fee_bp <= MAX_TAKER_FEE_BP @ ErrorCode::InvalidPlatformFeeBp,
        bump,
    )]
    pub buyer_escrow_payment_account: UncheckedAccount<'info>,

    /// CHECK: check with contraints
    #[account(mut)]
    buyer_referral: UncheckedAccount<'info>,
    /// CHECK: check with contraints
    #[account(mut)]
    seller_referral: UncheckedAccount<'info>,

    /// CHECK: checked by address and in CPI
    #[account(address = mpl_token_metadata::ID)]
    token_metadata_program: UncheckedAccount<'info>,
    /// CHECK: checked in CPI
    edition: UncheckedAccount<'info>,
    /// CHECK: checked in CPI
    authorization_rules_program: UncheckedAccount<'info>,
    /// CHECK: checked in CPI
    authorization_rules: UncheckedAccount<'info>,
    /// CHECK: checked in CPI
    #[account(mut)]
    owner_token_record: UncheckedAccount<'info>,
    /// CHECK: checked in CPI
    #[account(mut)]
    destination_token_record: UncheckedAccount<'info>,
    /// CHECK: check in cpi
    #[account(address = sysvar::instructions::id())]
    instructions: UncheckedAccount<'info>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
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
}

pub fn handle_mip1_execute_sale<'info>(
    ctx: Context<'_, '_, '_, 'info, MIP1ExecuteSaleV2<'info>>,
    args: MIP1ExecuteSaleV2Args,
) -> Result<()> {
    let payer = &ctx.accounts.payer;
    let buyer = &ctx.accounts.buyer;
    let seller = &ctx.accounts.seller;
    let token_mint = &ctx.accounts.token_mint;
    let metadata = &ctx.accounts.metadata;
    let notary = &ctx.accounts.notary;
    let seller_trade_state = &ctx.accounts.seller_trade_state;
    let buyer_trade_state = &ctx.accounts.buyer_trade_state;
    let buyer_escrow_payment_account = &ctx.accounts.buyer_escrow_payment_account;
    let auction_house = &ctx.accounts.auction_house;
    let auction_house_key = auction_house.key();
    let auction_house_treasury = &ctx.accounts.auction_house_treasury;
    let token_account = &ctx.accounts.token_account;
    let buyer_receipt_token_account = &ctx.accounts.buyer_receipt_token_account;

    let program_as_signer = &ctx.accounts.program_as_signer;
    let edition = &ctx.accounts.edition;
    let authorization_rules_program = &ctx.accounts.authorization_rules_program;
    let authorization_rules = &ctx.accounts.authorization_rules;
    let owner_token_record = &ctx.accounts.owner_token_record;
    let destination_token_record = &ctx.accounts.destination_token_record;

    let associated_token_program = &ctx.accounts.associated_token_program;
    let token_program = &ctx.accounts.token_program;
    let system_program = &ctx.accounts.system_program;
    let instructions = &ctx.accounts.instructions;
    let remaining_accounts = ctx.remaining_accounts;

    if !buyer.is_signer && !seller.is_signer {
        return Err(ErrorCode::SaleRequiresSigner.into());
    }
    let taker = if buyer.is_signer { buyer } else { seller };

    let bid_args = BidArgs::from_account_info(buyer_trade_state)?;
    let is_spl = bid_args.payment_mint != Pubkey::default();
    bid_args.check_args(
        ctx.accounts.buyer_referral.key,
        args.price,
        &token_mint.key(),
        1,
        if is_spl {
            index_ra!(remaining_accounts, 0).key
        } else {
            &bid_args.payment_mint
        },
    )?;
    let sell_args = SellArgs::from_account_info(seller_trade_state)?;
    sell_args.check_args(
        ctx.accounts.seller_referral.key,
        &bid_args.buyer_price,
        &bid_args.token_mint,
        &1,
        &bid_args.payment_mint,
    )?;

    let clock = Clock::get()?;
    if bid_args.expiry.abs() > 1 && clock.unix_timestamp > bid_args.expiry.abs() {
        return Err(ErrorCode::InvalidExpiry.into());
    }
    if sell_args.expiry.abs() > 1 && clock.unix_timestamp > sell_args.expiry.abs() {
        return Err(ErrorCode::InvalidExpiry.into());
    }

    assert_metadata_valid(metadata, &token_mint.key())?;

    let program_as_signer_seeds = &[
        PREFIX.as_bytes(),
        SIGNER.as_bytes(),
        &[ctx.bumps.program_as_signer],
    ];
    let payload = Payload {
        map: HashMap::from([(
            "SourceSeeds".to_owned(),
            PayloadType::Seeds(SeedsVec {
                seeds: vec![PREFIX.as_bytes().to_vec(), SIGNER.as_bytes().to_vec()],
            }),
        )]),
    };
    let ins = TransferBuilder::new()
        .token(token_account.key())
        .token_owner(token_account.owner)
        .destination_token(buyer_receipt_token_account.key())
        .destination_owner(buyer.key())
        .mint(token_mint.key())
        .metadata(metadata.key())
        .edition(Some(edition.key()))
        .token_record(Some(owner_token_record.key()))
        .destination_token_record(Some(destination_token_record.key()))
        .authority(program_as_signer.key())
        .payer(payer.key())
        .system_program(system_program.key())
        .sysvar_instructions(instructions.key())
        .spl_token_program(token_program.key())
        .spl_ata_program(associated_token_program.key())
        .authorization_rules_program(Some(authorization_rules_program.key()))
        .authorization_rules(Some(authorization_rules.key()))
        .transfer_args(TransferArgs::V1 {
            authorization_data: Some(AuthorizationData { payload }),
            amount: 1,
        })
        .instruction();

    invoke_signed(
        &ins,
        &[
            program_as_signer.to_account_info(),
            token_account.to_account_info(),
            buyer_receipt_token_account.to_account_info(),
            buyer.to_account_info(),
            payer.to_account_info(),
            token_mint.to_account_info(),
            metadata.to_account_info(),
            edition.to_account_info(),
            token_program.to_account_info(),
            associated_token_program.to_account_info(),
            system_program.to_account_info(),
            instructions.to_account_info(),
            authorization_rules_program.to_account_info(),
            authorization_rules.to_account_info(),
            owner_token_record.to_account_info(),
            destination_token_record.to_account_info(),
            seller.to_account_info(), // might not be needed, but skips an unnecessary branch
        ],
        &[program_as_signer_seeds],
    )?;

    let buyer_escrow_signer_seeds: &[&[&[u8]]] = &[&[
        PREFIX.as_bytes(),
        auction_house_key.as_ref(),
        buyer.key.as_ref(),
        &[ctx.bumps.buyer_escrow_payment_account],
    ]];

    // buyer pays creator royalties
    let metadata_parsed = &Metadata::safe_deserialize(&metadata.data.borrow()).unwrap();
    let royalty = pay_creator_fees(
        &mut (if is_spl {
            remaining_accounts[4..].iter()
        } else {
            remaining_accounts.iter()
        }),
        None,
        metadata_parsed,
        &buyer_escrow_payment_account.to_account_info(),
        buyer_escrow_signer_seeds,
        args.price,
        10_000,
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
    )?;
    check_programmable(metadata_parsed)?;

    let (actual_maker_fee_bp, actual_taker_fee_bp) =
        get_actual_maker_taker_fee_bp(notary, args.maker_fee_bp, args.taker_fee_bp);
    let (maker_fee, taker_fee) = transfer_listing_payment(
        args.price,
        actual_maker_fee_bp,
        actual_taker_fee_bp,
        taker,
        seller,
        buyer_escrow_payment_account,
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
        buyer_escrow_signer_seeds,
    )?;

    // close token account
    if token_account.amount == 1 && token_account.owner == program_as_signer.key() {
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
            &[program_as_signer_seeds],
        )?;
    }

    assert_is_ata(
        buyer_receipt_token_account,
        &buyer.key(),
        &token_mint.key(),
        &buyer.key(),
    )?;

    try_close_buyer_escrow(
        buyer_escrow_payment_account,
        buyer,
        system_program,
        buyer_escrow_signer_seeds,
    )?;

    // we don't need to zero out buyer_trade_state, just copy zero discriminator to it and then close
    close_account_anchor(buyer_trade_state, buyer)?;
    close_account_anchor(seller_trade_state, seller)?;
    msg!(
        "{{\"maker_fee\":{},\"taker_fee\":{},\"royalty\":{},\"price\":{},\"seller_expiry\":{},\"buyer_expiry\":{}}}",
        maker_fee,
        taker_fee,
        royalty,
        args.price,
        sell_args.expiry,
        bid_args.expiry,
    );

    Ok(())
}
