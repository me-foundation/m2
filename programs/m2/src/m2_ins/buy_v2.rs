use anchor_lang::Discriminator;
use solana_program::{program::invoke, system_instruction};

use crate::index_ra;

use {
    crate::constants::*,
    crate::errors::ErrorCode,
    crate::states::*,
    crate::utils::*,
    anchor_lang::prelude::*,
    anchor_spl::token::{Mint, Token},
};

#[derive(Accounts)]
pub struct BuyV2<'info> {
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
    #[account(mut, seeds=[PREFIX.as_bytes(), auction_house.key().as_ref(), wallet.key().as_ref()], bump)]
    escrow_payment_account: UncheckedAccount<'info>,
    /// CHECK: authority
    authority: UncheckedAccount<'info>,
    #[account(seeds=[PREFIX.as_bytes(), auction_house.creator.as_ref()], bump=auction_house.bump, has_one=authority)]
    auction_house: Account<'info, AuctionHouse>,
    /// CHECK: seeds check + discriminator check
    #[account(
        mut,
        seeds=[
            PREFIX.as_bytes(),
            wallet.key().as_ref(),
            auction_house.key().as_ref(),
            token_mint.key().as_ref(),
        ],
        bump)]
    buyer_trade_state: AccountInfo<'info>,
    /// CHECK: buyer_referral
    buyer_referral: UncheckedAccount<'info>,
    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,
    // remaining accounts:
    // 0. payment_mint (optional) - if the buyer is paying in a token, this is the mint of that token
    // 1. payment_source_token_account (optional) - if the buyer is paying in a token, this is the source token account, we need to verify sufficient balance
    // ...
    // -1. payer (optional) - this wallet will try to subsidize SOL for the buyer if bidding in SOL, and will pay for bts rent
}

pub fn handle<'info>(
    ctx: Context<'_, '_, '_, 'info, BuyV2<'info>>,
    buyer_price: u64,
    token_size: u64,
    buyer_state_expiry: i64,
    buyer_creator_royalty_bp: u16,
    _extra_args: &[u8],
) -> Result<()> {
    let (remaining_accounts, possible_payer) =
        split_payer_from_remaining_accounts(ctx.remaining_accounts);
    let payer = if let Some(p) = possible_payer {
        p
    } else {
        &ctx.accounts.wallet
    };
    let metadata = &ctx.accounts.metadata;
    let token_mint = &ctx.accounts.token_mint;
    let escrow_payment_account = &ctx.accounts.escrow_payment_account;
    let auction_house = &ctx.accounts.auction_house;
    let buyer_referral = &ctx.accounts.buyer_referral;
    let buyer_trade_state = &ctx.accounts.buyer_trade_state;
    let system_program = &ctx.accounts.system_program;
    let is_spl = remaining_accounts.len() == 2;

    if buyer_trade_state.data_len() > 0 {
        let discriminator_data = &buyer_trade_state.try_borrow_data()?[0..8];
        if discriminator_data != BuyerTradeState::discriminator()
            && discriminator_data != BuyerTradeStateV2::discriminator()
        {
            return Err(ErrorCode::InvalidDiscriminator.into());
        }
    }

    if buyer_creator_royalty_bp > 10_000 {
        return Err(ErrorCode::InvalidBasisPoints.into());
    }

    if buyer_price > MAX_PRICE || buyer_price == 0 {
        return Err(ErrorCode::InvalidPrice.into());
    }

    if remaining_accounts.is_empty() {
        // SOL
        if escrow_payment_account.lamports() < buyer_price {
            let diff = buyer_price
                .checked_sub(escrow_payment_account.lamports())
                .ok_or(ErrorCode::NumericalOverflow)?;
            invoke(
                &system_instruction::transfer(payer.key, &escrow_payment_account.key(), diff),
                &[
                    payer.to_account_info(),
                    escrow_payment_account.to_account_info(),
                    system_program.to_account_info(),
                ],
            )?;
        }
    } else if is_spl {
        // SPL
        assert_payment_mint(index_ra!(remaining_accounts, 0))?;
        let payment_token_account_parsed = assert_is_ata(
            index_ra!(remaining_accounts, 1),
            escrow_payment_account.key,
            index_ra!(remaining_accounts, 0).key,
            escrow_payment_account.key,
        )?;
        if payment_token_account_parsed.amount < buyer_price {
            return Err(ErrorCode::InvalidTokenAmount.into());
        }
    } else {
        return Err(ErrorCode::InvalidAccountState.into());
    }

    assert_metadata_valid(metadata, &token_mint.key())?;
    let bts_bump = ctx.bumps.buyer_trade_state;
    // create or reallocate the buyer trade state
    // after this call the correct size should be allocated and discriminator should be written
    create_or_realloc_buyer_trade_state(
        buyer_trade_state,
        payer,
        &[
            PREFIX.as_bytes(),
            ctx.accounts.wallet.key().as_ref(),
            auction_house.key().as_ref(),
            token_mint.key().as_ref(),
            &[bts_bump],
        ],
    )?;

    let bts_v2 = BuyerTradeStateV2 {
        auction_house_key: auction_house.key(),
        buyer: ctx.accounts.wallet.key(),
        buyer_referral: buyer_referral.key(),
        buyer_price,
        token_mint: token_mint.key(),
        token_size,
        bump: bts_bump,
        buyer_creator_royalty_bp,
        expiry: get_default_buyer_state_expiry(buyer_state_expiry),
        payment_mint: if is_spl {
            index_ra!(remaining_accounts, 0).key()
        } else {
            Pubkey::default()
        },
    };

    // serialize
    let bts_v2_serialized = bts_v2.try_to_vec()?;
    buyer_trade_state.try_borrow_mut_data()?[8..8 + bts_v2_serialized.len()]
        .copy_from_slice(&bts_v2_serialized);
    msg!(
        "{{\"price\":{},\"buyer_expiry\":{}}}",
        bts_v2.buyer_price,
        bts_v2.expiry
    );
    Ok(())
}
