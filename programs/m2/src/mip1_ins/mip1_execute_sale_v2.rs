use mpl_token_auth_rules::payload::{Payload, PayloadType, SeedsVec};
use mpl_token_metadata::{
    instruction::{builders::TransferBuilder, InstructionBuilder},
    processor::AuthorizationData,
    state::{Metadata, TokenMetadataAccount},
};
use solana_program::{
    program::{invoke, invoke_signed},
    system_instruction, sysvar,
};

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
    price: u64,
    maker_fee_bp: i16,
    taker_fee_bp: u16,
}

#[derive(Accounts)]
#[instruction(args:MIP1ExecuteSaleV2Args)]
pub struct MIP1ExecuteSaleV2<'info> {
    #[account(
      mut,
      constraint = (payer.key == buyer.key || payer.key == seller.key) @ ErrorCode::SaleRequiresSigner,
    )]
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
    #[account(mut)]
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
    #[account(
        mut,
        close=seller,
        seeds=[
            PREFIX.as_bytes(),
            seller.key().as_ref(),
            auction_house.key().as_ref(),
            token_account.key().as_ref(),
            token_mint.key().as_ref(),
        ],
        bump,
        constraint= seller_trade_state.seller_referral == seller_referral.key(),
    )]
    pub seller_trade_state: Box<Account<'info, SellerTradeState>>,
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
    #[account(address = mpl_token_metadata::id())]
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
}

pub fn handle<'info>(
    ctx: Context<'_, '_, '_, 'info, MIP1ExecuteSaleV2<'info>>,
    args: MIP1ExecuteSaleV2Args,
) -> Result<()> {
    let payer = &ctx.accounts.payer;
    let buyer = &ctx.accounts.buyer;
    let seller = &ctx.accounts.seller;
    let token_mint = &ctx.accounts.token_mint;
    let metadata = &ctx.accounts.metadata;
    let notary = &ctx.accounts.notary;
    let seller_trade_state = &mut ctx.accounts.seller_trade_state;
    let buyer_trade_state = &mut ctx.accounts.buyer_trade_state;
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

    let bid_args = BidArgs::from_account_info(&buyer_trade_state.to_account_info())?;
    bid_args.check_args(
        ctx.accounts.buyer_referral.key,
        seller_trade_state.buyer_price,
        &seller_trade_state.token_mint,
        seller_trade_state.token_size,
    )?;
    bid_args.check_args(
        ctx.accounts.buyer_referral.key,
        args.price,
        &token_mint.key(),
        1,
    )?;

    let clock = Clock::get()?;
    if bid_args.expiry.abs() > 1 && clock.unix_timestamp > bid_args.expiry.abs() {
        return Err(ErrorCode::InvalidExpiry.into());
    }
    if seller_trade_state.expiry.abs() > 1 && clock.unix_timestamp > seller_trade_state.expiry.abs()
    {
        return Err(ErrorCode::InvalidExpiry.into());
    }

    assert_metadata_valid(metadata, &token_mint.key())?;

    let program_as_signer_seeds = &[
        PREFIX.as_bytes(),
        SIGNER.as_bytes(),
        &[*ctx.bumps.get("program_as_signer").unwrap()],
    ];
    let payload = Payload::from([(
        "SourceSeeds".to_owned(),
        PayloadType::Seeds(SeedsVec {
            seeds: vec![PREFIX.as_bytes().to_vec(), SIGNER.as_bytes().to_vec()],
        }),
    )]);
    let ins = TransferBuilder::new()
        .token(token_account.key())
        .token_owner(token_account.owner)
        .destination(buyer_receipt_token_account.key())
        .destination_owner(buyer.key())
        .mint(token_mint.key())
        .metadata(metadata.key())
        .edition(edition.key())
        .owner_token_record(owner_token_record.key())
        .destination_token_record(destination_token_record.key())
        .authority(program_as_signer.key())
        .payer(payer.key())
        .system_program(system_program.key())
        .sysvar_instructions(instructions.key())
        .spl_token_program(token_program.key())
        .spl_ata_program(associated_token_program.key())
        .authorization_rules_program(authorization_rules_program.key())
        .authorization_rules(authorization_rules.key())
        .build(mpl_token_metadata::instruction::TransferArgs::V1 {
            authorization_data: Some(AuthorizationData { payload }),
            amount: 1,
        })
        .unwrap()
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

    let buyer_escrow_signer_seeds = [
        PREFIX.as_bytes(),
        auction_house_key.as_ref(),
        buyer.key.as_ref(),
        &[*ctx.bumps.get("buyer_escrow_payment_account").unwrap()],
    ];

    // buyer pays creator royalties
    let metadata_parsed = &Metadata::from_account_info(metadata).unwrap();
    let royalty = pay_creator_fees(
        &mut ctx.remaining_accounts.iter(),
        None,
        metadata_parsed,
        &buyer_escrow_payment_account.to_account_info(),
        system_program,
        &buyer_escrow_signer_seeds,
        args.price,
        10_000,
    )?;
    check_programmable(metadata_parsed)?;

    // payer pays maker/taker fees
    // seller is payer and taker
    //   seller as payer pays (maker_fee + taker_fee) to treasury
    //   buyer as maker needs to pay args.price + maker_fee + royalty
    //   seller gets (args.price + maker_fee) from buyer
    // buyer is payer and taker
    //   buyer as payer pays (maker_fee + taker_fee) to treasury
    //   buyer as taker needs to pay (args.price + taker_fee + royalty)
    //   seller gets (args.price - maker_fee) from buyer
    let (actual_maker_fee_bp, actual_taker_fee_bp) =
        get_actual_maker_taker_fee_bp(notary, args.maker_fee_bp, args.taker_fee_bp);
    let maker_fee = (args.price as i128)
        .checked_mul(actual_maker_fee_bp as i128)
        .ok_or(ErrorCode::NumericalOverflow)?
        .checked_div(10000)
        .ok_or(ErrorCode::NumericalOverflow)? as i64;
    let taker_fee = (args.price as u128)
        .checked_mul(actual_taker_fee_bp as u128)
        .ok_or(ErrorCode::NumericalOverflow)?
        .checked_div(10000)
        .ok_or(ErrorCode::NumericalOverflow)? as u64;
    let seller_will_get_from_buyer = if payer.key.eq(seller.key) {
        (args.price as i64)
            .checked_add(maker_fee)
            .ok_or(ErrorCode::NumericalOverflow)?
    } else {
        (args.price as i64)
            .checked_sub(maker_fee)
            .ok_or(ErrorCode::NumericalOverflow)?
    } as u64;
    let total_platform_fee = (maker_fee
        .checked_add(taker_fee as i64)
        .ok_or(ErrorCode::NumericalOverflow)?) as u64;

    invoke_signed(
        &system_instruction::transfer(
            buyer_escrow_payment_account.key,
            seller.key,
            seller_will_get_from_buyer,
        ),
        &[
            buyer_escrow_payment_account.to_account_info(),
            seller.to_account_info(),
            system_program.to_account_info(),
        ],
        &[&buyer_escrow_signer_seeds],
    )?;

    if total_platform_fee > 0 {
        if payer.key.eq(seller.key) {
            invoke(
                &system_instruction::transfer(
                    payer.key,
                    auction_house_treasury.key,
                    total_platform_fee,
                ),
                &[
                    payer.to_account_info(),
                    auction_house_treasury.to_account_info(),
                    system_program.to_account_info(),
                ],
            )?;
        } else {
            invoke_signed(
                &system_instruction::transfer(
                    buyer_escrow_payment_account.key,
                    auction_house_treasury.key,
                    total_platform_fee,
                ),
                &[
                    buyer_escrow_payment_account.to_account_info(),
                    auction_house_treasury.to_account_info(),
                    system_program.to_account_info(),
                ],
                &[&buyer_escrow_signer_seeds],
            )?;
        }
    }

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
        &[&buyer_escrow_signer_seeds],
    )?;

    // zero-out the token_size so that we don't accidentally use it again
    seller_trade_state.token_size = 0;

    // we don't need to zero out buyer_trade_state, just copy zero discriminator to it and then close
    close_account_anchor(buyer_trade_state, buyer)?;
    msg!(
        "{{\"maker_fee\":{},\"taker_fee\":{},\"royalty\":{},\"price\":{},\"seller_expiry\":{},\"buyer_expiry\":{}}}",
        maker_fee,
        taker_fee,
        royalty,
        args.price,
        seller_trade_state.expiry,
        bid_args.expiry,
    );

    Ok(())
}
