use {
    crate::constants::*,
    crate::errors::ErrorCode,
    crate::states::*,
    crate::utils::*,
    anchor_lang::{
        prelude::*,
        solana_program::{program::invoke_signed, system_instruction},
    },
};

#[derive(Accounts)]
#[instruction(escrow_payment_bump: u8)]
pub struct Withdraw<'info> {
    /// CHECK: wallet
    #[account(mut)]
    wallet: UncheckedAccount<'info>,
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

pub fn handle<'info>(
    ctx: Context<'_, '_, '_, 'info, Withdraw<'info>>,
    escrow_payment_bump: u8,
    amount: u64,
) -> Result<()> {
    let wallet = &ctx.accounts.wallet;
    let escrow_payment_account = &ctx.accounts.escrow_payment_account;
    let authority = &ctx.accounts.authority;
    let auction_house = &ctx.accounts.auction_house;
    let system_program = &ctx.accounts.system_program;
    let auction_house_key = auction_house.key();
    let wallet_key = wallet.key();

    assert_bump(
        &[
            PREFIX.as_bytes(),
            auction_house.key().as_ref(),
            wallet.key().as_ref(),
        ],
        ctx.program_id,
        escrow_payment_bump,
    )?;

    if !wallet.is_signer && !authority.is_signer {
        return Err(ErrorCode::NoValidSignerPresent.into());
    }

    let escrow_signer_seeds = [
        PREFIX.as_bytes(),
        auction_house_key.as_ref(),
        wallet_key.as_ref(),
        &[escrow_payment_bump],
    ];

    invoke_signed(
        &system_instruction::transfer(&escrow_payment_account.key(), &wallet.key(), amount),
        &[
            escrow_payment_account.to_account_info(),
            wallet.to_account_info(),
            system_program.to_account_info(),
        ],
        &[&escrow_signer_seeds],
    )?;

    Ok(())
}
