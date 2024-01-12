use anchor_lang::Discriminator;
use solana_program::{program::invoke, system_instruction};

use {
    crate::constants::*,
    crate::errors::ErrorCode,
    crate::states::*,
    crate::utils::*,
    anchor_lang::{prelude::*, AnchorDeserialize},
    anchor_spl::token::{Mint, Token},
};

#[derive(Accounts)]
#[instruction(buyer_state_bump: u8, escrow_payment_bump: u8, buyer_price: u64, token_size: u64, buyer_state_expiry: i64)]
pub struct Buy<'info> {
    #[account(mut)]
    wallet: Signer<'info>,
    /// CHECK: notary is not dangerous because we don't read or write from this account
    notary: UncheckedAccount<'info>,
    #[account(
        constraint = token_mint.supply == 1 @ ErrorCode::InvalidTokenMint,
        constraint = token_mint.decimals == 0 @ ErrorCode::InvalidTokenMint
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
    /// CHECK: escrow_payment_account
    #[account(mut, seeds=[PREFIX.as_bytes(), auction_house.key().as_ref(), wallet.key().as_ref()], bump=escrow_payment_bump)]
    escrow_payment_account: UncheckedAccount<'info>,
    /// CHECK: authority
    authority: UncheckedAccount<'info>,
    #[account(seeds=[PREFIX.as_bytes(), auction_house.creator.as_ref()], bump=auction_house.bump, has_one=authority)]
    auction_house: Account<'info, AuctionHouse>,
    #[account(
        init_if_needed,
        payer=wallet,
        seeds=[
            PREFIX.as_bytes(),
            wallet.key().as_ref(),
            auction_house.key().as_ref(),
            token_mint.key().as_ref(),
        ],
        space=BuyerTradeState::LEN,
        bump)]
    buyer_trade_state: Box<Account<'info, BuyerTradeState>>,
    /// CHECK: buyer_referral
    buyer_referral: UncheckedAccount<'info>,
    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,
    rent: Sysvar<'info, Rent>,
}

pub fn handle<'info>(
    ctx: Context<'_, '_, '_, 'info, Buy<'info>>,
    escrow_payment_bump: u8,
    buyer_price: u64,
    token_size: u64,
    buyer_state_expiry: i64,
) -> Result<()> {
    let wallet = &ctx.accounts.wallet;
    let metadata = &ctx.accounts.metadata;
    let token_mint = &ctx.accounts.token_mint;
    let escrow_payment_account = &ctx.accounts.escrow_payment_account;
    let auction_house = &ctx.accounts.auction_house;
    let buyer_referral = &ctx.accounts.buyer_referral;
    let buyer_trade_state_clone = &ctx.accounts.buyer_trade_state.to_account_info();
    let buyer_trade_state = &mut ctx.accounts.buyer_trade_state;
    let system_program = &ctx.accounts.system_program;
    let auction_house_key = auction_house.key();

    let discriminator_ai = buyer_trade_state_clone.try_borrow_data()?;
    if discriminator_ai[..8] != BuyerTradeState::discriminator() && discriminator_ai[..8] != [0; 8]
    {
        return Err(ErrorCode::InvalidDiscriminator.into());
    }

    if buyer_price > MAX_PRICE || buyer_price == 0 {
        return Err(ErrorCode::InvalidPrice.into());
    }

    assert_bump(
        &[
            PREFIX.as_bytes(),
            auction_house.key().as_ref(),
            wallet.key().as_ref(),
        ],
        ctx.program_id,
        escrow_payment_bump,
    )?;

    if escrow_payment_account.lamports() < buyer_price {
        let diff = buyer_price
            .checked_sub(escrow_payment_account.lamports())
            .ok_or(ErrorCode::NumericalOverflow)?;
        invoke(
            &system_instruction::transfer(&wallet.key(), &escrow_payment_account.key(), diff),
            &[
                wallet.to_account_info(),
                escrow_payment_account.to_account_info(),
                system_program.to_account_info(),
            ],
        )?;
    }

    let token_mint_key = token_mint.key();
    assert_metadata_valid(metadata, &token_mint_key)?;
    buyer_trade_state.auction_house_key = auction_house_key;
    buyer_trade_state.buyer = wallet.key();
    buyer_trade_state.buyer_referral = buyer_referral.key();
    buyer_trade_state.buyer_price = buyer_price;
    buyer_trade_state.token_mint = token_mint_key;
    buyer_trade_state.token_size = token_size;
    buyer_trade_state.bump = ctx.bumps.buyer_trade_state;
    buyer_trade_state.expiry = get_default_buyer_state_expiry(buyer_state_expiry);
    msg!(
        "{{\"price\":{},\"buyer_expiry\":{}}}",
        buyer_trade_state.buyer_price,
        buyer_trade_state.expiry,
    );
    Ok(())
}
