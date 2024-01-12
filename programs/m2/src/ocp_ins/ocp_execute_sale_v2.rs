use anchor_spl::associated_token::AssociatedToken;
use mpl_token_metadata::accounts::Metadata;
use open_creator_protocol::state::Policy;
use solana_program::sysvar;

use {
    crate::constants::*,
    crate::errors::ErrorCode,
    crate::states::*,
    crate::utils::*,
    anchor_lang::prelude::*,
    anchor_spl::token::{Mint, Token, TokenAccount},
};

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct OCPExecuteSaleV2Args {
    price: u64,
    maker_fee_bp: i16,
    taker_fee_bp: u16,
}

#[derive(Accounts)]
#[instruction(args:OCPExecuteSaleV2Args)]
pub struct OCPExecuteSaleV2<'info> {
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
        token::authority = seller,
        constraint = seller_token_ata.amount == 1,
    )]
    pub seller_token_ata: Box<Account<'info, TokenAccount>>,
    /// CHECK: checked in cpi
    #[account(mut)]
    pub buyer_token_ata: UncheckedAccount<'info>,
    #[account(
        constraint = token_mint.supply == 1 && token_mint.decimals == 0,
    )]
    pub token_mint: Box<Account<'info, Mint>>,
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
            seller_token_ata.key().as_ref(),
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

    /// CHECK: check in cpi
    #[account(mut)]
    ocp_mint_state: UncheckedAccount<'info>,
    /// CHECK: check in cpi
    ocp_policy: Box<Account<'info, Policy>>,
    /// CHECK: check in cpi
    ocp_freeze_authority: UncheckedAccount<'info>,
    /// CHECK: check in cpi
    #[account(address = open_creator_protocol::id())]
    ocp_program: UncheckedAccount<'info>,
    /// CHECK: check in cpi
    #[account(address = community_managed_token::id())]
    cmt_program: UncheckedAccount<'info>,
    /// CHECK: check in cpi
    #[account(address = sysvar::instructions::id())]
    instructions: UncheckedAccount<'info>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

pub fn handle<'info>(
    ctx: Context<'_, '_, '_, 'info, OCPExecuteSaleV2<'info>>,
    args: OCPExecuteSaleV2Args,
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
    let system_program = &ctx.accounts.system_program;

    let bid_args = BidArgs::from_account_info(buyer_trade_state)?;
    bid_args.check_args(
        &bid_args.buyer_referral,
        args.price,
        &token_mint.key(),
        1,
        &Pubkey::default(),
    )?;
    let sell_args = SellArgs::from_account_info(seller_trade_state)?;
    sell_args.check_args(
        &sell_args.seller_referral,
        &args.price,
        &token_mint.key(),
        &1,
        &Pubkey::default(),
    )?;

    let clock = Clock::get()?;
    if bid_args.expiry.abs() > 1 && clock.unix_timestamp > bid_args.expiry.abs() {
        return Err(ErrorCode::InvalidExpiry.into());
    }
    if sell_args.expiry.abs() > 1 && clock.unix_timestamp > sell_args.expiry.abs() {
        return Err(ErrorCode::InvalidExpiry.into());
    }

    assert_metadata_valid(metadata, &token_mint.key())?;

    open_creator_protocol::cpi::unlock(CpiContext::new_with_signer(
        ctx.accounts.ocp_program.to_account_info(),
        open_creator_protocol::cpi::accounts::UnlockCtx {
            policy: ctx.accounts.ocp_policy.to_account_info(),
            mint: ctx.accounts.token_mint.to_account_info(),
            metadata: ctx.accounts.metadata.to_account_info(),
            mint_state: ctx.accounts.ocp_mint_state.to_account_info(),
            from: ctx.accounts.program_as_signer.to_account_info(),
            cmt_program: ctx.accounts.cmt_program.to_account_info(),
            instructions: ctx.accounts.instructions.to_account_info(),
        },
        &[&[
            PREFIX.as_bytes(),
            SIGNER.as_bytes(),
            &[ctx.bumps.program_as_signer],
        ]],
    ))?;

    if ctx.accounts.buyer_token_ata.data_is_empty() {
        open_creator_protocol::cpi::init_account(CpiContext::new(
            ctx.accounts.ocp_program.to_account_info(),
            open_creator_protocol::cpi::accounts::InitAccountCtx {
                policy: ctx.accounts.ocp_policy.to_account_info(),
                mint: ctx.accounts.token_mint.to_account_info(),
                metadata: ctx.accounts.metadata.to_account_info(),
                mint_state: ctx.accounts.ocp_mint_state.to_account_info(),
                from: ctx.accounts.buyer.to_account_info(),
                from_account: ctx.accounts.buyer_token_ata.to_account_info(),
                cmt_program: ctx.accounts.cmt_program.to_account_info(),
                instructions: ctx.accounts.instructions.to_account_info(),
                freeze_authority: ctx.accounts.ocp_freeze_authority.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
                associated_token_program: ctx.accounts.associated_token_program.to_account_info(),
                payer: ctx.accounts.payer.to_account_info(),
            },
        ))?;
    }

    open_creator_protocol::cpi::transfer(CpiContext::new_with_signer(
        ctx.accounts.ocp_program.to_account_info(),
        open_creator_protocol::cpi::accounts::TransferCtx {
            policy: ctx.accounts.ocp_policy.to_account_info(),
            mint: ctx.accounts.token_mint.to_account_info(),
            metadata: ctx.accounts.metadata.to_account_info(),
            mint_state: ctx.accounts.ocp_mint_state.to_account_info(),
            from: ctx.accounts.program_as_signer.to_account_info(),
            from_account: ctx.accounts.seller_token_ata.to_account_info(),
            cmt_program: ctx.accounts.cmt_program.to_account_info(),
            instructions: ctx.accounts.instructions.to_account_info(),
            freeze_authority: ctx.accounts.ocp_freeze_authority.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
            to: ctx.accounts.buyer.to_account_info(),
            to_account: ctx.accounts.buyer_token_ata.to_account_info(),
        },
        &[&[
            PREFIX.as_bytes(),
            SIGNER.as_bytes(),
            &[ctx.bumps.program_as_signer],
        ]],
    ))?;

    let buyer_escrow_signer_seeds: &[&[&[u8]]] = &[&[
        PREFIX.as_bytes(),
        auction_house_key.as_ref(),
        buyer.key.as_ref(),
        &[ctx.bumps.buyer_escrow_payment_account],
    ]];

    // buyer pays creator royalties
    let metadata_parsed = &Metadata::safe_deserialize(&metadata.data.borrow()).unwrap();
    let royalty = pay_creator_fees(
        &mut ctx.remaining_accounts.iter(),
        Some(&ctx.accounts.ocp_policy),
        metadata_parsed,
        &buyer_escrow_payment_account.to_account_info(),
        buyer_escrow_signer_seeds,
        args.price,
        10_000,
        None,
    )?;

    let (actual_maker_fee_bp, actual_taker_fee_bp) =
        get_actual_maker_taker_fee_bp(notary, args.maker_fee_bp, args.taker_fee_bp);
    let (maker_fee, taker_fee) = transfer_listing_payment(
        args.price,
        actual_maker_fee_bp,
        actual_taker_fee_bp,
        payer,
        seller,
        buyer_escrow_payment_account,
        auction_house_treasury,
        None,
        buyer_escrow_signer_seeds,
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
