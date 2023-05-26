use mpl_token_auth_rules::payload::{Payload, PayloadType, SeedsVec};
use mpl_token_metadata::{
    instruction::{builders::TransferBuilder, InstructionBuilder},
    pda::find_token_record_account,
    processor::AuthorizationData,
    state::{Metadata, TokenDelegateRole, TokenMetadataAccount, TokenState},
};
use solana_program::{program::invoke, sysvar};
use spl_associated_token_account::get_associated_token_address;

use {
    crate::constants::*,
    crate::errors::ErrorCode,
    crate::states::*,
    crate::utils::{
        assert_is_ata, check_programmable, close_account_anchor,
        get_delegate_info_and_token_state_from_token_record,
    },
    anchor_lang::{prelude::*, AnchorDeserialize},
    anchor_spl::{
        associated_token::AssociatedToken,
        token::{Mint, Token, TokenAccount},
    },
};

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct MIP1SellArgs {
    price: u64,
    expiry: i64,
}

#[derive(Accounts)]
#[instruction(args:MIP1SellArgs)]
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
    token_account: Account<'info, TokenAccount>,
    #[account(
        constraint = token_mint.supply == 1 && token_mint.decimals == 0,
    )]
    token_mint: Account<'info, Mint>,
    /// CHECK: check in cpi
    #[account(mut)]
    metadata: UncheckedAccount<'info>,
    #[account(
        seeds=[PREFIX.as_bytes(), auction_house.creator.as_ref()],
        constraint = auction_house.notary == notary.key(),
        bump,
    )]
    auction_house: Box<Account<'info, AuctionHouse>>,
    #[account(
        init_if_needed,
        payer=wallet,
        seeds=[
            PREFIX.as_bytes(),
            wallet.key().as_ref(),
            auction_house.key().as_ref(),
            token_ata.key().as_ref(),
            token_mint.key().as_ref(),
        ],
        constraint = args.price > 0 && args.price <= MAX_PRICE @ ErrorCode::InvalidPrice,
        constraint = args.expiry < 0 @ ErrorCode::InvalidExpiry,
        space=SellerTradeState::LEN,
        bump)]
    seller_trade_state: Box<Account<'info, SellerTradeState>>,
    /// CHECK: seeds checked, only should be used when migrating mip0->mip1
    #[account(
        init_if_needed,
        payer=wallet,
        seeds=[
            PREFIX.as_bytes(),
            wallet.key().as_ref(),
            auction_house.key().as_ref(),
            token_account.key().as_ref(),
            token_mint.key().as_ref(),
        ],
        space=SellerTradeState::LEN,
        bump
    )]
    migration_seller_trade_state: Box<Account<'info, SellerTradeState>>,
    /// CHECK: seller_referral
    seller_referral: UncheckedAccount<'info>,

    /// CHECK: token_ata is ata(program_as_signer, mint)
    ///   escrow mode for init sell:        we transfer from token_account to token_ata
    ///   escrow mode for change price:     token_account is the same as token_ata
    ///   migration mode for change price:  token_ata is not used, because we only need token_account which is owned by program_as_signer
    #[account(mut, address = get_associated_token_address(&program_as_signer.key(), &token_mint.key()))]
    token_ata: UncheckedAccount<'info>,
    /// CHECK: checked by address and in CPI
    #[account(address = mpl_token_metadata::id())]
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
    #[account(mut, address = find_token_record_account(&token_mint.key(), &token_account.key()).0)]
    owner_token_record: UncheckedAccount<'info>,
    /// CHECK: checked in CPI
    #[account(mut)]
    destination_token_record: UncheckedAccount<'info>,

    associated_token_program: Program<'info, AssociatedToken>,
    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,
    rent: Sysvar<'info, Rent>,
}

pub fn handle<'info>(
    ctx: Context<'_, '_, '_, 'info, MIP1Sell<'info>>,
    args: MIP1SellArgs,
) -> Result<()> {
    let wallet = &ctx.accounts.wallet;
    let token_mint = &ctx.accounts.token_mint;
    let token_program = &ctx.accounts.token_program;
    let associated_token_program = &ctx.accounts.associated_token_program;
    let system_program = &ctx.accounts.system_program;
    let instructions = &ctx.accounts.instructions;
    let program_as_signer = &ctx.accounts.program_as_signer;
    let token_account = &ctx.accounts.token_account;
    let escrow_ata = &ctx.accounts.token_ata;

    let seller_trade_state = &mut ctx.accounts.seller_trade_state;
    let seller_referral = &ctx.accounts.seller_referral;
    let auction_house = &ctx.accounts.auction_house;

    let metadata = &ctx.accounts.metadata;
    let edition = &ctx.accounts.edition;
    let authorization_rules_program = &ctx.accounts.authorization_rules_program;
    let authorization_rules = &ctx.accounts.authorization_rules;
    let owner_token_record = &ctx.accounts.owner_token_record;
    let destination_token_record = &ctx.accounts.destination_token_record;
    let migration_seller_trade_state = &mut ctx.accounts.migration_seller_trade_state;

    let wallet_key = wallet.key();
    let token_mint_key = token_mint.key();
    let token_account_key = token_account.key();

    // can't set the existing seller_trade_state to another auction house
    if seller_trade_state.auction_house_key.ne(&Pubkey::default())
        && seller_trade_state
            .auction_house_key
            .ne(&auction_house.key())
        || migration_seller_trade_state
            .auction_house_key
            .ne(&Pubkey::default())
            && migration_seller_trade_state
                .auction_house_key
                .ne(&auction_house.key())
    {
        return Err(ErrorCode::InvalidAccountState.into());
    }

    check_programmable(&Metadata::from_account_info(metadata).unwrap())?;

    let (sts_to_modify, sts_to_close, escrow_account_key) =
        if token_account.owner == program_as_signer.key() {
            // we expect this to be change price for (escrow mode or migration mode)
            if token_account.amount != 1 || migration_seller_trade_state.seller != wallet.key() {
                msg!(
                    "unexpected amount {} or seller {}",
                    token_account.amount,
                    migration_seller_trade_state.seller
                );
                return Err(ErrorCode::InvalidAccountState.into());
            }
            (
                migration_seller_trade_state,
                seller_trade_state,
                token_account.key(),
            )
        } else {
            // seller currently owns the token - we need to check whether we want to escrow or not
            let (mut delegate, delegate_role, token_state) =
                get_delegate_info_and_token_state_from_token_record(owner_token_record)?;
            if let Some(delegate_key) = delegate {
                if delegate_key == program_as_signer.key() && token_state == TokenState::Unlocked {
                    // we treat this as if it is a new listing
                    delegate = None;
                }
            }
            match delegate {
                None => {
                    let payload = Payload::from([(
                        "DestinationSeeds".to_owned(),
                        PayloadType::Seeds(SeedsVec {
                            seeds: vec![PREFIX.as_bytes().to_vec(), SIGNER.as_bytes().to_vec()],
                        }),
                    )]);
                    // new listing - escrow token and modify seller_trade_state
                    let ins = TransferBuilder::new()
                        .token(token_account_key)
                        .token_owner(wallet_key)
                        .destination(escrow_ata.key())
                        .destination_owner(program_as_signer.key())
                        .mint(token_mint_key)
                        .metadata(metadata.key())
                        .edition(edition.key())
                        .owner_token_record(owner_token_record.key())
                        .destination_token_record(destination_token_record.key())
                        .authority(wallet_key)
                        .payer(wallet_key)
                        .system_program(system_program.key())
                        .sysvar_instructions(instructions.key())
                        .spl_token_program(token_program.key())
                        .spl_ata_program(associated_token_program.key())
                        .authorization_rules_program(authorization_rules_program.key())
                        .authorization_rules(authorization_rules.key())
                        .build(mpl_token_metadata::instruction::TransferArgs::V1 {
                            authorization_data: Some(AuthorizationData { payload }),
                            amount: 1,
                        })
                        .unwrap()
                        .instruction();
                    invoke(
                        &ins,
                        &[
                            wallet.to_account_info(),
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
                        &program_as_signer.key(),
                        &token_mint.key(),
                        &program_as_signer.key(),
                    )?;

                    (
                        seller_trade_state,
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

    sts_to_modify.auction_house_key = auction_house.key();
    sts_to_modify.seller = wallet_key;
    sts_to_modify.seller_referral = seller_referral.key();
    sts_to_modify.buyer_price = args.price;
    sts_to_modify.token_mint = token_mint_key;
    sts_to_modify.token_account = escrow_account_key;
    sts_to_modify.token_size = 1;
    sts_to_modify.bump = *ctx.bumps.get("seller_trade_state").unwrap();
    sts_to_modify.expiry = args.expiry; // negative number means non-movable listing mode

    msg!(
        "mip1_sell: {{\"seller_trade_state\":\"{}\",\"token_account\":\"{}\"}}",
        sts_to_modify.key(),
        escrow_account_key
    );
    msg!(
        "{{\"price\":{},\"seller_expiry\":{}}}",
        sts_to_modify.buyer_price,
        sts_to_modify.expiry
    );

    if sts_to_close.key() != sts_to_modify.key() {
        close_account_anchor(&sts_to_close.to_account_info(), wallet)?;
    }
    Ok(())
}
