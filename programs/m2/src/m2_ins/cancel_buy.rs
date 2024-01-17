use {
    crate::constants::*, crate::errors::ErrorCode, crate::states::*,
    crate::utils::close_account_anchor, anchor_lang::prelude::*, anchor_spl::token::Mint,
};

#[derive(Accounts)]
pub struct CancelBuy<'info> {
    /// CHECK: wallet
    #[account(mut)]
    wallet: UncheckedAccount<'info>,
    /// CHECK: notary is not dangerous because we don't read or write from this account
    notary: UncheckedAccount<'info>,
    #[account(mut)]
    token_mint: Account<'info, Mint>,
    /// CHECK: authority
    authority: UncheckedAccount<'info>,
    #[account(seeds=[PREFIX.as_bytes(), auction_house.creator.as_ref()], bump=auction_house.bump, has_one=authority)]
    auction_house: Account<'info, AuctionHouse>,
    /// CHECK: check bid_args
    #[account(
        mut,
        seeds=[
            PREFIX.as_bytes(),
            wallet.key().as_ref(),
            auction_house.key().as_ref(),
            token_mint.key().as_ref(),
        ],
        bump
    )]
    buyer_trade_state: AccountInfo<'info>,
    /// CHECK: buyer_referral
    buyer_referral: UncheckedAccount<'info>,
}

pub fn handle<'info>(
    ctx: Context<'_, '_, '_, 'info, CancelBuy<'info>>,
    buyer_price: u64,
    token_size: u64,
    buyer_state_expiry: i64,
) -> Result<()> {
    let wallet = &ctx.accounts.wallet;
    let notary = &ctx.accounts.notary;
    let buyer_trade_state = &mut ctx.accounts.buyer_trade_state;

    if buyer_trade_state.data_is_empty() {
        return Err(ErrorCode::EmptyTradeState.into());
    }

    let bid_args = BidArgs::from_account_info(buyer_trade_state)?;
    bid_args.check_args(
        ctx.accounts.buyer_referral.key,
        buyer_price,
        &bid_args.token_mint,
        token_size,
        &bid_args.payment_mint, // don't care about payment mint here
    )?;
    if bid_args.expiry != buyer_state_expiry {
        return Err(ErrorCode::InvalidExpiry.into());
    }

    // If wallet doesn't sign, notary must be CANCEL_AUTHORITY and also sign.
    let cancel_authority_signed = notary.is_signer && *notary.key == CANCEL_AUTHORITY;

    if !wallet.is_signer && !cancel_authority_signed {
        return Err(ErrorCode::NoValidSignerPresent.into());
    }

    close_account_anchor(buyer_trade_state, wallet)?;

    Ok(())
}
