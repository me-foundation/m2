use anchor_lang::Discriminator;

use crate::index_ra;

use {
    crate::constants::*,
    crate::errors::ErrorCode,
    crate::states::*,
    crate::utils::*,
    anchor_lang::prelude::*,
    anchor_spl::{
        associated_token::AssociatedToken,
        token::{Mint, SetAuthority, Token, TokenAccount},
    },
    spl_token::instruction::AuthorityType,
};

#[derive(Accounts)]
pub struct Sell<'info> {
    #[account(mut)]
    wallet: Signer<'info>,
    /// CHECK: notary is not dangerous because we don't read or write from this account
    notary: UncheckedAccount<'info>,
    /// CHECK: token_account is the account that holds the token, not necessarily the same as ata due to legacy reasons in M1
    #[account(mut, constraint= token_account.mint == token_mint.key())]
    token_account: Account<'info, TokenAccount>,
    /// CHECK: token_ata is the account that will hold the token after ata creation and setAuthority from wallet to program_as_signer
    #[account(mut)]
    token_ata: UncheckedAccount<'info>,
    #[account(
        constraint = token_mint.supply == 1 @ ErrorCode::InvalidTokenMint,
        constraint = token_mint.decimals == 0 @ ErrorCode::InvalidTokenMint,
    )]
    token_mint: Account<'info, Mint>,
    /// CHECK: metadata
    metadata: UncheckedAccount<'info>,
    /// CHECK: authority
    authority: UncheckedAccount<'info>,
    #[account(
      seeds=[PREFIX.as_bytes(), auction_house.creator.as_ref()],
      has_one=authority,
      bump,
    )]
    auction_house: Account<'info, AuctionHouse>,
    /// CHECK: checked in seeds
    #[account(
        mut,
        seeds=[
            PREFIX.as_bytes(),
            wallet.key().as_ref(),
            auction_house.key().as_ref(),
            token_ata.key().as_ref(),
            token_mint.key().as_ref(),
        ],
        bump
    )]
    seller_trade_state: UncheckedAccount<'info>,
    /// CHECK: seller_referral
    seller_referral: UncheckedAccount<'info>,
    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,
    ata_program: Program<'info, AssociatedToken>,
    /// CHECK: program_as_signer
    #[account(seeds=[PREFIX.as_bytes(), SIGNER.as_bytes()], bump)]
    program_as_signer: UncheckedAccount<'info>,
    rent: Sysvar<'info, Rent>,
    // remaining accounts:
    // 0. payment_mint (optional) - if the seller wants payment in a SPL token, this is the mint of that token
    // ...
    // -1. payer (optional) - this wallet will try to pay for sts rent
}

pub fn handle<'info>(
    ctx: Context<'_, '_, '_, 'info, Sell<'info>>,
    _program_as_signer_bump: u8,
    buyer_price: u64,
    token_size: u64,
    seller_state_expiry: i64,
) -> Result<()> {
    let wallet = &ctx.accounts.wallet;
    let (remaining_accounts, possible_payer) =
        split_payer_from_remaining_accounts(ctx.remaining_accounts);
    let payer = if let Some(p) = possible_payer {
        p
    } else {
        wallet
    };
    let token_mint = &ctx.accounts.token_mint;
    let metadata = &ctx.accounts.metadata;
    let seller_trade_state = &ctx.accounts.seller_trade_state;
    let seller_referral = &ctx.accounts.seller_referral;
    let auction_house = &ctx.accounts.auction_house;
    let token_program = &ctx.accounts.token_program;
    let system_program = &ctx.accounts.system_program;
    let program_as_signer = &ctx.accounts.program_as_signer;
    let token_ata = &ctx.accounts.token_ata;
    let token_account = &ctx.accounts.token_account;
    let payment_mint = if remaining_accounts.len() == 1 {
        assert_payment_mint(index_ra!(remaining_accounts, 0))?;
        Some(index_ra!(remaining_accounts, 0))
    } else {
        None
    };

    let token_ata_ai = token_ata.as_ref() as &AccountInfo;
    let token_account_ai = token_account.as_ref() as &AccountInfo;

    if !seller_trade_state.data_is_empty() {
        let discriminator_ai = seller_trade_state.try_borrow_data()?;
        if discriminator_ai[..8] != SellerTradeState::discriminator()
            && discriminator_ai[..8] != SellerTradeStateV2::discriminator()
        {
            return Err(ErrorCode::InvalidDiscriminator.into());
        }
    }
    if token_size > token_account.amount || token_size == 0 {
        return Err(ErrorCode::InvalidTokenAmount.into());
    }
    if buyer_price > MAX_PRICE || buyer_price == 0 {
        return Err(ErrorCode::InvalidPrice.into());
    }
    if token_account_ai.key != token_ata_ai.key {
        transfer_token(
            &1,
            payer,
            wallet,
            wallet,
            None,
            DestinationSpecifier::Ai(wallet),
            token_mint.as_ref(),
            token_account.as_ref(),
            token_ata,
            token_program,
            system_program,
            Some(program_as_signer.key),
            &[],
        )?;
    }
    assert_metadata_valid(metadata, &token_mint.key())?;

    // seller_state_expiry < 0, non-movable listing mode
    //   - with program_as_signer to hold the authority
    //   - the sts will be closed when delist
    if seller_state_expiry >= 0 {
        return Err(ErrorCode::InvalidExpiry.into());
    }
    if !is_token_owner(token_ata_ai, program_as_signer.key)? {
        anchor_spl::token::set_authority(
            CpiContext::new(
                token_program.to_account_info(),
                SetAuthority {
                    account_or_mint: token_ata_ai.to_account_info(),
                    current_authority: wallet.to_account_info(),
                },
            ),
            AuthorityType::AccountOwner,
            Some(program_as_signer.key()),
        )?;
    } else if seller_trade_state.data_is_empty() {
        // so token owner is already program_as_signer, but token_size is 0
        // this is likely a relist from other auction house, not change sell price, we should simply block it
        return Err(ErrorCode::InvalidAccountState.into());
    }

    create_or_realloc_seller_trade_state(
        seller_trade_state,
        payer,
        &[
            PREFIX.as_bytes(),
            wallet.key().as_ref(),
            auction_house.key().as_ref(),
            token_ata.key().as_ref(),
            token_mint.key().as_ref(),
            &[ctx.bumps.seller_trade_state],
        ],
    )?;
    let sts = SellerTradeStateV2 {
        auction_house_key: auction_house.key(),
        seller: wallet.key(),
        seller_referral: seller_referral.key(),
        buyer_price,
        token_mint: token_mint.key(),
        token_account: token_ata_ai.key(),
        token_size,
        bump: ctx.bumps.seller_trade_state,
        expiry: seller_state_expiry,
        payment_mint: if let Some(m) = payment_mint {
            *m.key
        } else {
            Pubkey::default()
        },
    };
    let sts_v2_serialized = sts.try_to_vec()?;
    seller_trade_state.try_borrow_mut_data()?[8..8 + sts_v2_serialized.len()]
        .copy_from_slice(&sts_v2_serialized);

    msg!(
        "{{\"price\":{},\"seller_expiry\":{}}}",
        buyer_price,
        seller_state_expiry
    );
    Ok(())
}
