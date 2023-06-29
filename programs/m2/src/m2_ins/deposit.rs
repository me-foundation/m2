use solana_program::program::invoke;
use std::cmp;
use {
    crate::constants::*,
    crate::states::*,
    anchor_lang::{prelude::*, solana_program::system_instruction},
};

#[derive(Accounts)]
#[instruction(escrow_payment_bump: u8)]
pub struct Deposit<'info> {
    #[account(mut)]
    wallet: Signer<'info>,
    /// CHECK: notary is not dangerous because we don't read or write from this account
    notary: UncheckedAccount<'info>,
    /// CHECK: escrow_payment_account
    #[account(mut, seeds=[PREFIX.as_bytes(), auction_house.key().as_ref(), wallet.key().as_ref()], bump=escrow_payment_bump)]
    escrow_payment_account: UncheckedAccount<'info>,
    /// CHECK: authority
    authority: UncheckedAccount<'info>,
    #[account(seeds=[PREFIX.as_bytes(), auction_house.creator.as_ref()], bump=auction_house.bump, has_one=authority)]
    auction_house: Account<'info, AuctionHouse>,
    system_program: Program<'info, System>,
}

pub fn handle<'info>(ctx: Context<'_, '_, '_, 'info, Deposit<'info>>, amount: u64) -> Result<()> {
    let wallet = &ctx.accounts.wallet;
    let escrow_payment_account = &ctx.accounts.escrow_payment_account;
    let system_program = &ctx.accounts.system_program;

    invoke(
        &system_instruction::transfer(
            &wallet.key(),
            &escrow_payment_account.key(),
            cmp::max(amount, Rent::get()?.minimum_balance(0)),
        ),
        &[
            escrow_payment_account.to_account_info(),
            wallet.to_account_info(),
            system_program.to_account_info(),
        ],
    )?;

    Ok(())
}
