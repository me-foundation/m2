use solana_program::sysvar;

use {
    crate::constants::*,
    crate::errors::ErrorCode,
    crate::states::*,
    anchor_lang::prelude::*,
    anchor_spl::token::{Mint, Token, TokenAccount},
};

#[derive(Accounts)]
pub struct OCPCancelSell<'info> {
    /// CHECK: will check this in code
    #[account(mut)]
    wallet: UncheckedAccount<'info>,
    notary: Signer<'info>,
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
    #[account(
        seeds=[PREFIX.as_bytes(), auction_house.creator.as_ref()],
        bump,
    )]
    auction_house: Account<'info, AuctionHouse>,
    #[account(
        mut,
        close=wallet,
        seeds=[
            PREFIX.as_bytes(),
            wallet.key().as_ref(),
            auction_house.key().as_ref(),
            token_ata.key().as_ref(),
            token_mint.key().as_ref(),
        ],
        bump)]
    seller_trade_state: Box<Account<'info, SellerTradeState>>,

    /// CHECK: check in cpi
    #[account(mut)]
    ocp_mint_state: UncheckedAccount<'info>,
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

pub fn handle<'info>(ctx: Context<'_, '_, '_, 'info, OCPCancelSell<'info>>) -> Result<()> {
    let notary = &ctx.accounts.notary;
    let wallet = &ctx.accounts.wallet;

    let cancel_authority_signed = *notary.key == CANCEL_AUTHORITY;
    let auction_house_notary_signed = *notary.key == ctx.accounts.auction_house.notary;

    if !wallet.is_signer && !cancel_authority_signed {
        return Err(ErrorCode::NoValidSignerPresent.into());
    }

    if wallet.is_signer && !auction_house_notary_signed {
        return Err(ErrorCode::NoValidSignerPresent.into());
    }

    let seller_trade_state = &mut ctx.accounts.seller_trade_state;

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

    if wallet.is_signer {
        open_creator_protocol::cpi::revoke(CpiContext::new(
            ctx.accounts.ocp_program.to_account_info(),
            open_creator_protocol::cpi::accounts::RevokeCtx {
                policy: ctx.accounts.ocp_policy.to_account_info(),
                mint: ctx.accounts.token_mint.to_account_info(),
                metadata: ctx.accounts.metadata.to_account_info(),
                mint_state: ctx.accounts.ocp_mint_state.to_account_info(),
                from: ctx.accounts.wallet.to_account_info(),
                from_account: ctx.accounts.token_ata.to_account_info(),
                instructions: ctx.accounts.instructions.to_account_info(),
                freeze_authority: ctx.accounts.ocp_freeze_authority.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
                cmt_program: ctx.accounts.cmt_program.to_account_info(),
            },
        ))?;
    }

    msg!(
        "{{\"price\":{},\"seller_expiry\":{}}}",
        seller_trade_state.buyer_price,
        seller_trade_state.expiry
    );
    Ok(())
}
