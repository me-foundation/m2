use solana_program::program::invoke;
use std::cmp;

use crate::{
    index_ra,
    utils::{split_payer_from_remaining_accounts, DestinationSpecifier},
};

use {
    crate::constants::*,
    crate::errors::ErrorCode,
    crate::states::*,
    crate::utils::{assert_keys_equal, assert_payment_mint, transfer_token},
    anchor_lang::{prelude::*, solana_program::system_instruction},
};

#[derive(Accounts)]
pub struct Deposit<'info> {
    /// CHECK: seeds check, this is the beneficiary of the deposit
    #[account(mut)]
    wallet: UncheckedAccount<'info>,
    /// CHECK: notary is not dangerous because we don't read or write from this account
    notary: UncheckedAccount<'info>,
    /// CHECK: escrow_payment_account
    #[account(mut, seeds=[PREFIX.as_bytes(), auction_house.key().as_ref(), wallet.key().as_ref()], bump)]
    escrow_payment_account: UncheckedAccount<'info>,
    /// CHECK: authority
    authority: UncheckedAccount<'info>,
    #[account(seeds=[PREFIX.as_bytes(), auction_house.creator.as_ref()], bump=auction_house.bump, has_one=authority)]
    auction_house: Account<'info, AuctionHouse>,
    system_program: Program<'info, System>,
    // remaining accounts:
    // 0. payment_mint (optional) - if included, must be a valid token mint
    // 1. deposit_source_token_account (optional)
    // 2. deposit_destination_token_account (optional)
    // 3. token_program (optional)
    // 4. associated_token_program (optional)
    // ...
    // -1. payer (optional) - but either payer or wallet must be signer
}

pub fn handle<'info>(ctx: Context<'_, '_, '_, 'info, Deposit<'info>>, amount: u64) -> Result<()> {
    let (remaining_accounts, possible_payer) =
        split_payer_from_remaining_accounts(ctx.remaining_accounts);
    if !ctx.accounts.wallet.is_signer && possible_payer.is_none() {
        return Err(ErrorCode::NoValidSignerPresent.into());
    }
    let payer = if let Some(payer) = possible_payer {
        payer
    } else {
        &ctx.accounts.wallet
    };
    let escrow_payment_account = &ctx.accounts.escrow_payment_account;
    let system_program = &ctx.accounts.system_program;

    if remaining_accounts.is_empty() {
        invoke(
            &system_instruction::transfer(
                payer.key,
                &escrow_payment_account.key(),
                cmp::max(amount, Rent::get()?.minimum_balance(0)),
            ),
            &[
                escrow_payment_account.to_account_info(),
                payer.to_account_info(),
                system_program.to_account_info(),
            ],
        )?;
    } else {
        assert_keys_equal(index_ra!(remaining_accounts, 3).key, &spl_token::id())?;
        assert_payment_mint(index_ra!(remaining_accounts, 0))?;
        transfer_token(
            &amount,
            payer,
            payer,
            payer,
            None,
            DestinationSpecifier::Ai(escrow_payment_account),
            index_ra!(remaining_accounts, 0),
            index_ra!(remaining_accounts, 1),
            index_ra!(remaining_accounts, 2),
            index_ra!(remaining_accounts, 3),
            system_program,
            None,
            &[],
        )?;
    }

    Ok(())
}
