use std::str::FromStr;

use solana_program::program::invoke;
use spl_token::instruction::revoke;

use {
    crate::constants::*,
    crate::errors::ErrorCode,
    crate::states::*,
    crate::utils::*,
    anchor_lang::{prelude::*, AnchorDeserialize},
    anchor_spl::token::{Mint, SetAuthority, Token, TokenAccount},
    spl_token::instruction::AuthorityType,
};

#[derive(Accounts)]
#[instruction(buyer_price: u64, token_size: u64, seller_state_expiry: i64)]
pub struct CancelSell<'info> {
    /// CHECK: wallet must sign, otherwise delist authority (notary) must sign
    #[account(mut)]
    wallet: UncheckedAccount<'info>,
    /// CHECK: notary is not dangerous because we don't read or write from this account
    notary: UncheckedAccount<'info>,
    #[account(mut)]
    token_account: Account<'info, TokenAccount>,
    token_mint: Account<'info, Mint>,
    /// CHECK: authority
    authority: UncheckedAccount<'info>,
    #[account(seeds=[PREFIX.as_bytes(), auction_house.creator.as_ref()], bump=auction_house.bump, has_one=authority)]
    auction_house: Account<'info, AuctionHouse>,
    #[account(mut,
      close=wallet,
      constraint= seller_trade_state.buyer_price == buyer_price,
      constraint= seller_trade_state.token_size == token_size,
      constraint= seller_trade_state.expiry == seller_state_expiry,
      constraint= seller_trade_state.seller_referral == seller_referral.key(),
      seeds=[
        PREFIX.as_bytes(),
        wallet.key().as_ref(),
        auction_house.key().as_ref(),
        token_account.key().as_ref(),
        token_mint.key().as_ref(),
      ], bump=seller_trade_state.bump)]
    seller_trade_state: Box<Account<'info, SellerTradeState>>,
    /// CHECK: seller_referral
    seller_referral: UncheckedAccount<'info>,
    token_program: Program<'info, Token>,
}

pub fn handle<'info>(
    ctx: Context<'_, '_, '_, 'info, CancelSell<'info>>,
    seller_state_expiry: i64,
) -> Result<()> {
    let wallet = &ctx.accounts.wallet;
    let token_account = &ctx.accounts.token_account;
    let token_mint = &ctx.accounts.token_mint;
    let seller_trade_state = &mut ctx.accounts.seller_trade_state;
    let token_program = &ctx.accounts.token_program;
    let remaining_accounts = &ctx.remaining_accounts;
    let notary = &ctx.accounts.notary;
    let auction_house = &ctx.accounts.auction_house;

    // If wallet doesn't sign, notary must be CANCEL_AUTHORITY and also sign.
    let cancel_authority_signed =
        notary.is_signer && *notary.key == Pubkey::from_str(CANCEL_AUTHORITY).unwrap();

    if !wallet.is_signer && !cancel_authority_signed {
        return Err(ErrorCode::NoValidSignerPresent.into());
    }

    if !cancel_authority_signed {
        assert_valid_notary(
            auction_house,
            notary,
            100u8, // 100% enforced cosign
        )?;
    }
    assert_keys_equal(token_mint.key(), token_account.mint)?;
    if seller_trade_state.to_account_info().data_is_empty() {
        return Err(ErrorCode::EmptyTradeState.into());
    }

    // If seller_state_expiry is negative, we treat it that program_as_signer is the authority
    // For max compatibility, we derive the authority from the first remaining accounts.
    if seller_state_expiry < 0 {
        if remaining_accounts.is_empty() {
            return Err(ErrorCode::InvalidRemainingAccountsWithoutProgramAsSigner.into());
        }

        let (program_as_signer, wallet_bump) =
            Pubkey::find_program_address(&[PREFIX.as_bytes(), SIGNER.as_bytes()], ctx.program_id);
        if remaining_accounts[0].key() != program_as_signer {
            return Err(ErrorCode::InvalidRemainingAccountsWithoutProgramAsSigner.into());
        }
        let seeds = &[PREFIX.as_bytes(), SIGNER.as_bytes(), &[wallet_bump][..]];
        anchor_spl::token::set_authority(
            CpiContext::new(
                token_program.to_account_info(),
                SetAuthority {
                    account_or_mint: token_account.to_account_info(),
                    current_authority: remaining_accounts[0].clone(),
                },
            )
            .with_signer(&[&seeds[..]]),
            AuthorityType::AccountOwner,
            Some(wallet.key()),
        )?;
    }

    if seller_state_expiry >= 0 && token_account.owner == wallet.key() {
        invoke(
            &revoke(
                &token_program.key(),
                &token_account.key(),
                &wallet.key(),
                &[],
            )
            .unwrap(),
            &[
                token_program.to_account_info(),
                token_account.to_account_info(),
                wallet.to_account_info(),
            ],
        )?;
    }

    // zero-out the token size so that it cannot be used again
    seller_trade_state.token_size = 0;

    Ok(())
}
