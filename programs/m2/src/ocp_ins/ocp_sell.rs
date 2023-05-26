use open_creator_protocol::state::MintState;
use solana_program::sysvar;

use {
    crate::constants::*,
    crate::errors::ErrorCode,
    crate::states::*,
    anchor_lang::{prelude::*, AnchorDeserialize},
    anchor_spl::token::{Mint, Token, TokenAccount},
};

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct OCPSellArgs {
    price: u64,
    expiry: i64,
}

#[derive(Accounts)]
#[instruction(args:OCPSellArgs)]
pub struct OCPSell<'info> {
    #[account(mut)]
    wallet: Signer<'info>,
    /// CHECK: optional
    notary: UncheckedAccount<'info>,
    /// CHECK: program_as_signer
    #[account(seeds=[PREFIX.as_bytes(), SIGNER.as_bytes()], bump)]
    program_as_signer: UncheckedAccount<'info>,
    #[account(
        mut,
        token::mint = token_mint,
        token::authority = wallet,
        constraint = token_ata.amount == 1,
    )]
    token_ata: Account<'info, TokenAccount>,
    #[account(
        constraint = token_mint.supply == 1 && token_mint.decimals == 0,
    )]
    token_mint: Account<'info, Mint>,
    /// CHECK: check in cpi
    metadata: UncheckedAccount<'info>,
    #[account(
        seeds=[PREFIX.as_bytes(), auction_house.creator.as_ref()],
        constraint = auction_house.notary == notary.key(),
        bump,
    )]
    auction_house: Account<'info, AuctionHouse>,
    #[account(
        init_if_needed,
        payer=wallet,
        seeds=[
            PREFIX.as_bytes(),
            wallet.key().as_ref(),
            auction_house.key().as_ref(),
            token_ata.key().as_ref(),
            token_mint.key().as_ref(),
        ],
        constraint = args.price > 0 && args.price <= MAX_PRICE @ ErrorCode::InvalidPrice,
        constraint = args.expiry < 0 @ ErrorCode::InvalidExpiry,
        space=SellerTradeState::LEN,
        bump)]
    seller_trade_state: Box<Account<'info, SellerTradeState>>,
    /// CHECK: seller_referral
    seller_referral: UncheckedAccount<'info>,

    /// CHECK: check in cpi
    #[account(mut)]
    ocp_mint_state: Box<Account<'info, MintState>>,
    /// CHECK: check in cpi
    ocp_policy: UncheckedAccount<'info>,
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

    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,
    rent: Sysvar<'info, Rent>,
}

pub fn handle<'info>(
    ctx: Context<'_, '_, '_, 'info, OCPSell<'info>>,
    args: OCPSellArgs,
) -> Result<()> {
    let wallet = ctx.accounts.wallet.to_account_info();
    let token_mint = ctx.accounts.token_mint.to_account_info();
    let token_program = ctx.accounts.token_program.to_account_info();
    let program_as_signer = ctx.accounts.program_as_signer.to_account_info();
    let token_ata = ctx.accounts.token_ata.to_account_info();

    let seller_trade_state = &mut ctx.accounts.seller_trade_state;
    let seller_referral = &ctx.accounts.seller_referral;
    let auction_house = &ctx.accounts.auction_house;

    let wallet_key = wallet.key();
    let token_mint_key = token_mint.key();
    let token_ata_key = token_ata.key();

    // can't set the existing seller_trade_state to another auction house
    if seller_trade_state.auction_house_key.ne(&Pubkey::default())
        && seller_trade_state
            .auction_house_key
            .ne(&auction_house.key())
    {
        return Err(ErrorCode::InvalidAccountState.into());
    }

    match ctx.accounts.ocp_mint_state.locked_by {
        None => {
            open_creator_protocol::cpi::approve(CpiContext::new(
                ctx.accounts.ocp_program.to_account_info(),
                open_creator_protocol::cpi::accounts::ApproveCtx {
                    policy: ctx.accounts.ocp_policy.to_account_info(),
                    freeze_authority: ctx.accounts.ocp_freeze_authority.to_account_info(),
                    mint: token_mint,
                    mint_state: ctx.accounts.ocp_mint_state.to_account_info(),
                    metadata: ctx.accounts.metadata.to_account_info(),
                    from: wallet,
                    from_account: token_ata,
                    to: program_as_signer,
                    token_program,
                    cmt_program: ctx.accounts.cmt_program.to_account_info(),
                    instructions: ctx.accounts.instructions.to_account_info(),
                },
            ))?;
            open_creator_protocol::cpi::lock(CpiContext::new(
                ctx.accounts.ocp_program.to_account_info(),
                open_creator_protocol::cpi::accounts::LockCtx {
                    policy: ctx.accounts.ocp_policy.to_account_info(),
                    mint: ctx.accounts.token_mint.to_account_info(),
                    mint_state: ctx.accounts.ocp_mint_state.to_account_info(),
                    metadata: ctx.accounts.metadata.to_account_info(),
                    from: ctx.accounts.wallet.to_account_info(),
                    from_account: ctx.accounts.token_ata.to_account_info(),
                    to: ctx.accounts.program_as_signer.to_account_info(),
                    cmt_program: ctx.accounts.cmt_program.to_account_info(),
                    instructions: ctx.accounts.instructions.to_account_info(),
                },
            ))?;
        }
        Some(locked_by) => {
            if locked_by.ne(&program_as_signer.key()) || seller_trade_state.token_size == 0 {
                // if locked_by is not program_as_signer, but locked, we should return error

                // if locked_by is already program_as_signer, but token_size is 0
                // this is likely a relist from other auction house, not change sell price, we should simply block it
                return Err(ErrorCode::InvalidAccountState.into());
            }
        }
    }

    seller_trade_state.auction_house_key = auction_house.key();
    seller_trade_state.seller = wallet_key;
    seller_trade_state.seller_referral = seller_referral.key();
    seller_trade_state.buyer_price = args.price;
    seller_trade_state.token_mint = token_mint_key;
    seller_trade_state.token_account = token_ata_key;
    seller_trade_state.token_size = 1;
    seller_trade_state.bump = *ctx.bumps.get("seller_trade_state").unwrap();
    seller_trade_state.expiry = args.expiry; // negative number means non-movable listing mode

    msg!(
        "{{\"price\":{},\"seller_expiry\":{}}}",
        seller_trade_state.buyer_price,
        seller_trade_state.expiry
    );
    Ok(())
}
