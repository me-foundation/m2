use solana_program::native_token::LAMPORTS_PER_SOL;

use {
    crate::constants::*,
    crate::errors::ErrorCode,
    crate::states::*,
    anchor_lang::{
        prelude::*,
        solana_program::{program::invoke_signed, system_instruction},
    },
};

const MIN_LEFTOVER: u64 = LAMPORTS_PER_SOL; // 1 SOL

// WithdrawFromTreasury becomes a permissionless instruction
// that can be called by anyone. As long as the treasury_withdrawal_destination and amount is set correctly
#[derive(Accounts)]
pub struct WithdrawFromTreasury<'info> {
    /// CHECK: treasury_withdrawal_destination
    #[account(mut)]
    treasury_withdrawal_destination: UncheckedAccount<'info>,
    /// CHECK: auction_house_treasury
    #[account(
      mut,
      seeds=[PREFIX.as_bytes(), auction_house.key().as_ref(), TREASURY.as_bytes()],
      bump,
    )]
    auction_house_treasury: UncheckedAccount<'info>,
    #[account(
      seeds=[PREFIX.as_bytes(), auction_house.creator.as_ref()],
      bump,
      has_one=treasury_withdrawal_destination,
      has_one=auction_house_treasury,
    )]
    auction_house: Account<'info, AuctionHouse>,
    system_program: Program<'info, System>,
}

pub fn handle<'info>(
    ctx: Context<'_, '_, '_, 'info, WithdrawFromTreasury<'info>>,
    amount: u64,
) -> Result<()> {
    let treasury_withdrawal_destination = &ctx.accounts.treasury_withdrawal_destination;
    let auction_house_treasury = &ctx.accounts.auction_house_treasury;
    let auction_house = &ctx.accounts.auction_house;
    let system_program = &ctx.accounts.system_program;

    // need to keep at least MIN_LEFTOVER in the treasury
    if amount
        > (auction_house_treasury
            .lamports()
            .checked_sub(MIN_LEFTOVER)
            .ok_or(ErrorCode::NumericalOverflow)?)
    {
        return Err(ErrorCode::InvalidAccountState.into());
    }

    let ah_key = auction_house.key();
    let auction_house_treasury_seeds = [
        PREFIX.as_bytes(),
        ah_key.as_ref(),
        TREASURY.as_bytes(),
        &[ctx.bumps.auction_house_treasury],
    ];
    invoke_signed(
        &system_instruction::transfer(
            &auction_house_treasury.key(),
            &treasury_withdrawal_destination.key(),
            amount,
        ),
        &[
            auction_house_treasury.to_account_info(),
            treasury_withdrawal_destination.to_account_info(),
            system_program.to_account_info(),
        ],
        &[&auction_house_treasury_seeds],
    )?;

    Ok(())
}
