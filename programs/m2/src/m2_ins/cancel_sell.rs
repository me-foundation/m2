use solana_program::program::invoke;
use spl_token::instruction::revoke;

use {
    crate::constants::*,
    crate::errors::ErrorCode,
    crate::states::*,
    crate::utils::*,
    anchor_lang::prelude::*,
    anchor_spl::token::{Mint, SetAuthority, Token, TokenAccount},
    spl_token::instruction::AuthorityType,
};

#[derive(Accounts)]
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
    /// CHECK: seeds check and check sell_args
    #[account(
        mut,
        seeds=[
          PREFIX.as_bytes(),
          wallet.key().as_ref(),
          auction_house.key().as_ref(),
          token_account.key().as_ref(),
          token_mint.key().as_ref(),
        ],
        bump
    )]
    seller_trade_state: AccountInfo<'info>,
    /// CHECK: seller_referral
    seller_referral: UncheckedAccount<'info>,
    token_program: Program<'info, Token>,
}

pub fn handle<'info>(
    ctx: Context<'_, '_, '_, 'info, CancelSell<'info>>,
    _buyer_price: u64,
    token_size: u64,
    seller_state_expiry: i64,
) -> Result<()> {
    let wallet = &ctx.accounts.wallet;
    let token_account = &ctx.accounts.token_account;
    let token_mint = ctx.accounts.token_mint.as_ref() as &AccountInfo;
    let seller_trade_state = &ctx.accounts.seller_trade_state;
    let token_program = &ctx.accounts.token_program;
    let notary = &ctx.accounts.notary;
    let auction_house = &ctx.accounts.auction_house;

    let sell_args = SellArgs::from_account_info(seller_trade_state)?;
    sell_args.check_args(
        ctx.accounts.seller_referral.key,
        &sell_args.buyer_price,
        token_mint.key,
        &token_size,
        &sell_args.payment_mint, // don't care about payment mint here
    )?;
    if sell_args.expiry != seller_state_expiry {
        return Err(ErrorCode::InvalidExpiry.into());
    }

    // If wallet doesn't sign, notary must be CANCEL_AUTHORITY and also sign.
    let cancel_authority_signed = notary.is_signer && *notary.key == CANCEL_AUTHORITY;

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
    assert_keys_equal(token_mint.key, &token_account.mint)?;
    if seller_trade_state.to_account_info().data_is_empty() {
        return Err(ErrorCode::EmptyTradeState.into());
    }

    // If seller_state_expiry is negative, we treat it that program_as_signer is the authority
    // For max compatibility, we derive the authority from the first remaining accounts.
    if seller_state_expiry < 0 {
        if ctx.remaining_accounts.is_empty() {
            return Err(ErrorCode::InvalidRemainingAccountsWithoutProgramAsSigner.into());
        }

        let (program_as_signer, wallet_bump) =
            Pubkey::find_program_address(&[PREFIX.as_bytes(), SIGNER.as_bytes()], ctx.program_id);
        if ctx.remaining_accounts[0].key() != program_as_signer {
            return Err(ErrorCode::InvalidRemainingAccountsWithoutProgramAsSigner.into());
        }
        let seeds = &[PREFIX.as_bytes(), SIGNER.as_bytes(), &[wallet_bump][..]];
        anchor_spl::token::set_authority(
            CpiContext::new(
                token_program.to_account_info(),
                SetAuthority {
                    account_or_mint: token_account.to_account_info(),
                    current_authority: ctx.remaining_accounts[0].clone(),
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
    close_account_anchor(seller_trade_state, wallet)?;

    Ok(())
}
