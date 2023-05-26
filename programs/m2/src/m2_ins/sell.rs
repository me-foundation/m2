use anchor_lang::Discriminator;
use solana_program::program::invoke;

use {
    crate::constants::*,
    crate::errors::ErrorCode,
    crate::states::*,
    crate::utils::*,
    anchor_lang::{prelude::*, AnchorDeserialize},
    anchor_spl::{
        associated_token::AssociatedToken,
        token::{Mint, SetAuthority, Token, TokenAccount},
    },
    spl_token::instruction::AuthorityType,
};

#[derive(Accounts)]
#[instruction(seller_state_bump: u8, program_as_signer_bump: u8, buyer_price: u64, token_size: u64, seller_state_expiry: i64)]
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
    #[account(
        init_if_needed,
        constraint= token_account.mint == token_mint.key(),
        payer=wallet,
        seeds=[
            PREFIX.as_bytes(),
            wallet.key().as_ref(),
            auction_house.key().as_ref(),
            token_ata.key().as_ref(),
            token_mint.key().as_ref(),
        ],
        space=SellerTradeState::LEN,
        bump)]
    seller_trade_state: Box<Account<'info, SellerTradeState>>,
    /// CHECK: seller_referral
    seller_referral: UncheckedAccount<'info>,
    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,
    ata_program: Program<'info, AssociatedToken>,
    /// CHECK: program_as_signer
    #[account(seeds=[PREFIX.as_bytes(), SIGNER.as_bytes()], bump)]
    program_as_signer: UncheckedAccount<'info>,
    rent: Sysvar<'info, Rent>,
}

pub fn handle<'info>(
    ctx: Context<'_, '_, '_, 'info, Sell<'info>>,
    _program_as_signer_bump: u8,
    buyer_price: u64,
    token_size: u64,
    seller_state_expiry: i64,
) -> Result<()> {
    let wallet = &ctx.accounts.wallet;
    let token_mint = &ctx.accounts.token_mint;
    let metadata = &ctx.accounts.metadata;
    let seller_trade_state_clone = &ctx.accounts.seller_trade_state.to_account_info();
    let seller_trade_state = &mut ctx.accounts.seller_trade_state;
    let seller_referral = &ctx.accounts.seller_referral;
    let auction_house = &ctx.accounts.auction_house;
    let token_program = &ctx.accounts.token_program;
    let ata_program = &ctx.accounts.ata_program;
    let system_program = &ctx.accounts.system_program;
    let rent = &ctx.accounts.rent;
    let program_as_signer = &ctx.accounts.program_as_signer;
    let token_ata = &ctx.accounts.token_ata;
    let token_account = &ctx.accounts.token_account;

    let token_ata_clone = &ctx.accounts.token_ata.to_account_info();
    let token_account_clone = &ctx.accounts.token_account.to_account_info();

    let discriminator_ai = seller_trade_state_clone.try_borrow_data()?;
    if discriminator_ai[..8] != SellerTradeState::discriminator() && discriminator_ai[..8] != [0; 8]
    {
        return Err(ErrorCode::InvalidDiscriminator.into());
    }

    if token_size > token_account.amount || token_size == 0 {
        return Err(ErrorCode::InvalidTokenAmount.into());
    }
    if buyer_price > MAX_PRICE || buyer_price == 0 {
        return Err(ErrorCode::InvalidPrice.into());
    }
    if token_account_clone.key != token_ata_clone.key {
        if token_ata.data_is_empty() {
            make_ata(
                token_ata.to_account_info(),
                wallet.to_account_info(),
                wallet.to_account_info(),
                token_mint.to_account_info(),
                ata_program.to_account_info(),
                token_program.to_account_info(),
                system_program.to_account_info(),
                rent.to_account_info(),
            )?;
        }

        invoke(
            &spl_token::instruction::transfer(
                token_program.key,
                &token_account.key(),
                &token_ata.key(),
                &wallet.key(),
                &[],
                token_size,
            )?,
            &[
                token_account_clone.to_account_info(),
                token_ata_clone.to_account_info(),
                wallet.to_account_info(),
                token_program.to_account_info(),
            ],
        )?;

        if token_size == token_account.amount {
            invoke(
                &spl_token::instruction::close_account(
                    token_program.key,
                    &token_account.key(),
                    &wallet.key(),
                    &wallet.key(),
                    &[],
                )?,
                &[
                    token_account_clone.to_account_info(),
                    wallet.to_account_info(),
                    wallet.to_account_info(),
                    token_program.to_account_info(),
                ],
            )?;
        }
    }

    assert_is_ata(
        token_ata_clone,
        &wallet.key(),
        &token_mint.key(),
        &program_as_signer.key(),
    )?;
    assert_metadata_valid(metadata, &token_mint.key())?;

    // seller_state_expiry < 0, non-movable listing mode
    //   - with program_as_signer to hold the authority
    //   - the sts will be closed when delist
    if seller_state_expiry >= 0 {
        return Err(ErrorCode::InvalidExpiry.into());
    }
    if !is_token_owner(token_ata_clone, &program_as_signer.key())? {
        anchor_spl::token::set_authority(
            CpiContext::new(
                token_program.to_account_info(),
                SetAuthority {
                    account_or_mint: token_ata_clone.to_account_info(),
                    current_authority: wallet.to_account_info(),
                },
            ),
            AuthorityType::AccountOwner,
            Some(program_as_signer.key()),
        )?;
    } else if seller_trade_state.token_size == 0 {
        // so token owner is already program_as_signer, but token_size is 0
        // this is likely a relist from other auction house, not change sell price, we should simply block it
        return Err(ErrorCode::InvalidAccountState.into());
    }

    seller_trade_state.auction_house_key = auction_house.key();
    seller_trade_state.seller = wallet.key();
    seller_trade_state.seller_referral = seller_referral.key();
    seller_trade_state.buyer_price = buyer_price;
    seller_trade_state.token_mint = token_account.mint;
    seller_trade_state.token_account = token_ata_clone.key();
    seller_trade_state.token_size = token_size;
    seller_trade_state.expiry = seller_state_expiry;
    seller_trade_state.bump = *ctx.bumps.get("seller_trade_state").unwrap();

    msg!(
        "{{\"price\":{},\"seller_expiry\":{}}}",
        buyer_price,
        seller_state_expiry
    );
    Ok(())
}
