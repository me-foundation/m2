use std::collections::HashMap;

use mpl_token_metadata::{
    accounts::{Metadata, TokenRecord},
    instructions::TransferBuilder,
    types::{
        AuthorizationData, Payload, PayloadType, SeedsVec, TokenDelegateRole, TokenState,
        TransferArgs,
    },
};
use solana_program::{program::invoke, sysvar};
use spl_associated_token_account::get_associated_token_address;

use crate::index_ra;

use {
    crate::constants::*,
    crate::errors::ErrorCode,
    crate::states::*,
    crate::utils::{
        assert_is_ata, assert_payment_mint, check_programmable, close_account_anchor,
        create_or_realloc_seller_trade_state, get_delegate_info_and_token_state_from_token_record,
        split_payer_from_remaining_accounts,
    },
    anchor_lang::{prelude::*, AnchorDeserialize, AnchorSerialize},
    anchor_spl::{
        associated_token::AssociatedToken,
        token::{Mint, Token, TokenAccount},
    },
};

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct MIP1SellArgs {
    pub price: u64,
    pub expiry: i64,
}

#[derive(Accounts)]
pub struct MIP1Sell<'info> {
    #[account(mut)]
    wallet: Signer<'info>,
    /// CHECK: optional
    notary: UncheckedAccount<'info>,
    /// CHECK: program_as_signer
    #[account(seeds=[PREFIX.as_bytes(), SIGNER.as_bytes()], bump)]
    program_as_signer: UncheckedAccount<'info>,
    #[account(
        mut,
        token::mint = token_mint,
        constraint = token_account.owner == wallet.key() || token_account.owner == program_as_signer.key() @ ErrorCode::IncorrectOwner
    )]
    token_account: Box<Account<'info, TokenAccount>>,
    #[account(
        constraint = token_mint.supply == 1 && token_mint.decimals == 0,
    )]
    token_mint: Box<Account<'info, Mint>>,
    /// CHECK: check in cpi
    #[account(
    mut,
    seeds = [
        "metadata".as_bytes(),
        mpl_token_metadata::ID.as_ref(),
        token_mint.key().as_ref(),
    ],
    bump,
    seeds::program = mpl_token_metadata::ID,
    )]
    metadata: UncheckedAccount<'info>,
    #[account(
        seeds=[PREFIX.as_bytes(), auction_house.creator.as_ref()],
        constraint = auction_house.notary == notary.key(),
        bump,
    )]
    auction_house: Box<Account<'info, AuctionHouse>>,
    /// CHECK: seeds check and args check
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
    seller_trade_state: AccountInfo<'info>,
    /// CHECK: seeds checked, only should be used when migrating mip0->mip1
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
    migration_seller_trade_state: AccountInfo<'info>,
    /// CHECK: seller_referral
    seller_referral: UncheckedAccount<'info>,

    /// CHECK: token_ata is ata(program_as_signer, mint)
    ///   escrow mode for init sell:        we transfer from token_account to token_ata
    ///   escrow mode for change price:     token_account is the same as token_ata
    ///   migration mode for change price:  token_ata is not used, because we only need token_account which is owned by program_as_signer
    #[account(mut, address = get_associated_token_address(&program_as_signer.key(), &token_mint.key()))]
    token_ata: UncheckedAccount<'info>,
    /// CHECK: checked by address and in CPI
    #[account(address = mpl_token_metadata::ID)]
    token_metadata_program: UncheckedAccount<'info>,
    /// CHECK: checked in CPI
    edition: UncheckedAccount<'info>,
    /// CHECK: checked in CPI
    authorization_rules_program: UncheckedAccount<'info>,
    /// CHECK: checked in CPI
    authorization_rules: UncheckedAccount<'info>,
    /// CHECK: check in cpi
    #[account(address = sysvar::instructions::id())]
    instructions: UncheckedAccount<'info>,
    /// CHECK: checked in CPI
    #[account(mut, address = TokenRecord::find_pda(&token_mint.key(), &token_account.key()).0)]
    owner_token_record: UncheckedAccount<'info>,
    /// CHECK: checked in CPI
    #[account(mut)]
    destination_token_record: UncheckedAccount<'info>,

    associated_token_program: Program<'info, AssociatedToken>,
    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,
    rent: Sysvar<'info, Rent>,
    // remaining accounts:
    // 0. payment_mint (optional) - if the seller wants payment in a SPL token, this is the mint of that token
    // ...
    // -1. payer (optional) - this wallet will try to pay for sts rent
}

pub fn handle_mip1_sell<'info>(
    ctx: Context<'_, '_, '_, 'info, MIP1Sell<'info>>,
    args: &MIP1SellArgs,
) -> Result<()> {
    let (remaining_accounts, possible_payer) =
        split_payer_from_remaining_accounts(ctx.remaining_accounts);
    let wallet = &ctx.accounts.wallet;
    let payer = if let Some(p) = possible_payer {
        p
    } else {
        wallet
    };
    let token_mint = ctx.accounts.token_mint.as_ref().as_ref() as &AccountInfo;
    let token_program = &ctx.accounts.token_program;
    let associated_token_program = &ctx.accounts.associated_token_program;
    let system_program = &ctx.accounts.system_program;
    let instructions = &ctx.accounts.instructions;
    let program_as_signer = &ctx.accounts.program_as_signer;
    let token_account = &ctx.accounts.token_account;
    let escrow_ata = &ctx.accounts.token_ata;

    let seller_trade_state = &ctx.accounts.seller_trade_state;
    let seller_referral = &ctx.accounts.seller_referral;
    let auction_house = ctx.accounts.auction_house.as_ref().as_ref() as &AccountInfo;

    let metadata = &ctx.accounts.metadata;
    let edition = &ctx.accounts.edition;
    let authorization_rules_program = &ctx.accounts.authorization_rules_program;
    let authorization_rules = &ctx.accounts.authorization_rules;
    let owner_token_record = &ctx.accounts.owner_token_record;
    let destination_token_record = &ctx.accounts.destination_token_record;
    let migration_seller_trade_state = &ctx.accounts.migration_seller_trade_state;

    let wallet_key = wallet.key();
    let token_account_key = token_account.key();

    if args.price > MAX_PRICE || args.price == 0 {
        return Err(ErrorCode::InvalidPrice.into());
    }
    if args.expiry >= 0 {
        return Err(ErrorCode::InvalidExpiry.into());
    }

    // not too pretty, but needed to preserve original init_if_needed behavior
    let (sell_args, migration_sell_args) =
        if seller_trade_state.key == migration_seller_trade_state.key {
            let ret = if seller_trade_state.data_is_empty() {
                (Box::<SellArgs>::default(), Box::<SellArgs>::default())
            } else {
                let sell_args = Box::new(SellArgs::from_account_info(seller_trade_state)?);
                (sell_args.clone(), sell_args)
            };
            let sts_seeds: &[&[u8]] = &[
                PREFIX.as_bytes(),
                wallet.key.as_ref(),
                auction_house.key.as_ref(),
                escrow_ata.key.as_ref(),
                token_mint.key.as_ref(),
                &[ctx.bumps.seller_trade_state],
            ];
            create_or_realloc_seller_trade_state(seller_trade_state, payer, sts_seeds)?;
            ret
        } else {
            let sell_args = if seller_trade_state.data_is_empty() {
                Box::<SellArgs>::default()
            } else {
                Box::new(SellArgs::from_account_info(seller_trade_state)?)
            };
            let migration_sell_args = if migration_seller_trade_state.data_is_empty() {
                Box::<SellArgs>::default()
            } else {
                Box::new(SellArgs::from_account_info(migration_seller_trade_state)?)
            };

            let sts_seeds: &[&[u8]] = &[
                PREFIX.as_bytes(),
                wallet.key.as_ref(),
                auction_house.key.as_ref(),
                token_account_key.as_ref(),
                token_mint.key.as_ref(),
                &[ctx.bumps.migration_seller_trade_state],
            ];
            create_or_realloc_seller_trade_state(migration_seller_trade_state, payer, sts_seeds)?;
            let sts_seeds: &[&[u8]] = &[
                PREFIX.as_bytes(),
                wallet.key.as_ref(),
                auction_house.key.as_ref(),
                escrow_ata.key.as_ref(),
                token_mint.key.as_ref(),
                &[ctx.bumps.seller_trade_state],
            ];
            create_or_realloc_seller_trade_state(seller_trade_state, payer, sts_seeds)?;
            (sell_args, migration_sell_args)
        };

    // can't set the existing seller_trade_state to another auction house
    if sell_args.auction_house_key.ne(&Pubkey::default())
        && sell_args.auction_house_key.ne(auction_house.key)
        || migration_sell_args.auction_house_key.ne(&Pubkey::default())
            && migration_sell_args.auction_house_key.ne(auction_house.key)
    {
        return Err(ErrorCode::InvalidAccountState.into());
    }

    check_programmable(&Metadata::safe_deserialize(&metadata.data.borrow()).unwrap())?;

    let (sts_to_modify, sts_to_modify_bump, sts_to_close, escrow_account_key) =
        if token_account.owner == *program_as_signer.key {
            // we expect this to be change price for (escrow mode or migration mode)
            if token_account.amount != 1 || migration_sell_args.seller != wallet.key() {
                msg!(
                    "unexpected amount {} or seller {}",
                    token_account.amount,
                    migration_sell_args.seller
                );
                return Err(ErrorCode::InvalidAccountState.into());
            }
            (
                migration_seller_trade_state,
                ctx.bumps.migration_seller_trade_state,
                seller_trade_state,
                token_account.key(),
            )
        } else {
            // seller currently owns the token - we need to check whether we want to escrow or not
            let (mut delegate, delegate_role, token_state) =
                get_delegate_info_and_token_state_from_token_record(owner_token_record)?;
            if delegate.is_some() && token_state == TokenState::Unlocked {
                // we treat this as if it is a new listing since it should be transferrable
                delegate = None;
            }
            match delegate {
                None => {
                    let payload = Payload {
                        map: HashMap::from([(
                            "DestinationSeeds".to_owned(),
                            PayloadType::Seeds(SeedsVec {
                                seeds: vec![PREFIX.as_bytes().to_vec(), SIGNER.as_bytes().to_vec()],
                            }),
                        )]),
                    };
                    // new listing - escrow token and modify seller_trade_state
                    let ins = TransferBuilder::new()
                        .token(token_account_key)
                        .token_owner(wallet_key)
                        .destination_token(escrow_ata.key())
                        .destination_owner(program_as_signer.key())
                        .mint(token_mint.key())
                        .metadata(metadata.key())
                        .edition(Some(edition.key()))
                        .token_record(Some(owner_token_record.key()))
                        .destination_token_record(Some(destination_token_record.key()))
                        .authority(wallet_key)
                        .payer(payer.key())
                        .system_program(system_program.key())
                        .sysvar_instructions(instructions.key())
                        .spl_token_program(token_program.key())
                        .spl_ata_program(associated_token_program.key())
                        .authorization_rules_program(Some(authorization_rules_program.key()))
                        .authorization_rules(Some(authorization_rules.key()))
                        .transfer_args(TransferArgs::V1 {
                            authorization_data: Some(AuthorizationData { payload }),
                            amount: 1,
                        })
                        .instruction();
                    invoke(
                        &ins,
                        &[
                            wallet.to_account_info(),
                            payer.to_account_info(),
                            token_account.to_account_info(),
                            escrow_ata.to_account_info(),
                            program_as_signer.to_account_info(),
                            token_mint.to_account_info(),
                            metadata.to_account_info(),
                            edition.to_account_info(),
                            token_program.to_account_info(),
                            associated_token_program.to_account_info(),
                            system_program.to_account_info(),
                            instructions.to_account_info(),
                            authorization_rules_program.to_account_info(),
                            authorization_rules.to_account_info(),
                            owner_token_record.to_account_info(),
                            destination_token_record.to_account_info(),
                        ],
                    )?;

                    // close token account
                    if token_account.amount == 1 {
                        invoke(
                            &spl_token::instruction::close_account(
                                token_program.key,
                                &token_account.key(),
                                &wallet.key(),
                                &wallet.key(),
                                &[],
                            )?,
                            &[
                                token_account.to_account_info(),
                                wallet.to_account_info(),
                                token_program.to_account_info(),
                            ],
                        )?;
                    }

                    assert_is_ata(
                        escrow_ata,
                        program_as_signer.key,
                        token_mint.key,
                        program_as_signer.key,
                    )?;

                    (
                        seller_trade_state,
                        ctx.bumps.seller_trade_state,
                        migration_seller_trade_state,
                        escrow_ata.key(),
                    )
                }
                Some(delegate_key) => {
                    if delegate_key != program_as_signer.key() {
                        msg!("unexpected delegate: {}", delegate_key);
                        return Err(ErrorCode::InvalidAccountState.into());
                    }
                    if let Some(role) = delegate_role {
                        if role != TokenDelegateRole::Migration {
                            msg!("unexpected delegate role {:?}", role);
                            return Err(ErrorCode::InvalidAccountState.into());
                        }
                        // modify a previous escrowless listing - likely resulting from migration ocp -> mip1
                        (
                            migration_seller_trade_state,
                            ctx.bumps.migration_seller_trade_state,
                            seller_trade_state,
                            token_account.key(),
                        )
                    } else {
                        msg!("Delegate must have a role!");
                        return Err(ErrorCode::InvalidAccountState.into());
                    }
                }
            }
        };

    let payment_mint = if remaining_accounts.len() == 1 {
        assert_payment_mint(index_ra!(remaining_accounts, 0))?;
        index_ra!(remaining_accounts, 0).key()
    } else {
        Pubkey::default()
    };
    let sts = SellerTradeStateV2 {
        auction_house_key: auction_house.key(),
        seller: wallet_key,
        seller_referral: seller_referral.key(),
        buyer_price: args.price,
        token_mint: token_mint.key(),
        token_account: escrow_account_key,
        token_size: 1,
        bump: sts_to_modify_bump,
        expiry: args.expiry,
        payment_mint,
    };
    let sts_v2_serialized = sts.try_to_vec()?;
    sts_to_modify.try_borrow_mut_data()?[8..8 + sts_v2_serialized.len()]
        .copy_from_slice(&sts_v2_serialized);

    msg!(
        "mip1_sell: {{\"seller_trade_state\":\"{}\",\"token_account\":\"{}\"}}",
        sts_to_modify.key(),
        escrow_account_key
    );
    msg!(
        "{{\"price\":{},\"seller_expiry\":{}}}",
        sts.buyer_price,
        sts.expiry
    );

    if sts_to_close.key != sts_to_modify.key {
        close_account_anchor(sts_to_close, wallet)?;
    }
    Ok(())
}
