use crate::index_ra;

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
pub struct Withdraw<'info> {
    /// CHECK: wallet
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
    // 0. payment_mint (optional) - if included, will try to withdraw the token of this mint
    // 1. payment_source_token_account (optional) - token account controlled by escrow_payment_account that is source of tokens
    // 2. payment_destination_token_account (optional) - token account controlled by wallet that is destination of tokens
    // 3. token_program (optional)
    // 4. associated_token_program (optional)
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
    let remaining_accounts = ctx.remaining_accounts;

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

    let escrow_signer_seeds: &[&[&[u8]]] = &[&[
        PREFIX.as_bytes(),
        auction_house_key.as_ref(),
        wallet.key.as_ref(),
        &[escrow_payment_bump],
    ]];

    if ctx.remaining_accounts.is_empty() {
        invoke_signed(
            &system_instruction::transfer(&escrow_payment_account.key(), &wallet.key(), amount),
            &[
                escrow_payment_account.to_account_info(),
                wallet.to_account_info(),
                system_program.to_account_info(),
            ],
            escrow_signer_seeds,
        )?;
    } else {
        assert_keys_equal(index_ra!(remaining_accounts, 3).key, &spl_token::id())?;
        transfer_token(
            &amount,
            wallet,
            escrow_payment_account,
            wallet,
            None,
            DestinationSpecifier::Ai(wallet),
            index_ra!(remaining_accounts, 0),
            index_ra!(remaining_accounts, 1),
            index_ra!(remaining_accounts, 2),
            index_ra!(remaining_accounts, 3),
            system_program,
            None,
            escrow_signer_seeds,
        )?;
    }

    Ok(())
}
