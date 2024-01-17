use std::collections::HashMap;

use mpl_token_metadata::{
    accounts::Metadata,
    instructions::TransferBuilder,
    types::{AuthorizationData, Payload, PayloadType, SeedsVec, TransferArgs},
};
use solana_program::sysvar;
use spl_associated_token_account::get_associated_token_address;

use crate::utils::{assert_is_ata, check_programmable, close_account_anchor};
use {
    crate::constants::*,
    crate::errors::ErrorCode,
    crate::states::*,
    anchor_lang::prelude::*,
    anchor_spl::associated_token::AssociatedToken,
    anchor_spl::token::{set_authority, Mint, SetAuthority, Token, TokenAccount},
    solana_program::program::invoke_signed,
    spl_token::instruction::AuthorityType,
};

#[derive(Accounts)]
pub struct MIP1CancelSell<'info> {
    #[account(mut)]
    wallet: Signer<'info>,
    notary: Signer<'info>,
    /// CHECK: program_as_signer
    #[account(seeds=[PREFIX.as_bytes(), SIGNER.as_bytes()], bump)]
    program_as_signer: UncheckedAccount<'info>,
    #[account(
        mut,
        token::mint = token_mint,
        constraint = token_ata.amount == 1,
        constraint = token_ata.owner == wallet.key() || token_ata.owner == program_as_signer.key() @ ErrorCode::IncorrectOwner
    )]
    token_ata: Box<Account<'info, TokenAccount>>,
    #[account(
        constraint = token_mint.supply == 1 && token_mint.decimals == 0,
    )]
    token_mint: Account<'info, Mint>,
    /// CHECK: metadata
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
    /// CHECK: seeds check and check args
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
    /// CHECK: checked in CPI - account that will end up with the token
    /// should always be ATA of (mint, wallet)
    #[account(mut, address = get_associated_token_address(wallet.key, &token_mint.key()))]
    token_account: UncheckedAccount<'info>,
    /// CHECK: checked in CPI - temporary token account to facilitate MIP0 -> MIP1 migration
    /// should always be ATA of (mint, program_as_signer)
    #[account(mut, address = get_associated_token_address(program_as_signer.key, &token_mint.key()))]
    token_account_temp: UncheckedAccount<'info>,
    /// CHECK: checked in CPI - avoids unnecessary branch by passing in the toke record for token_account_temp as well
    #[account(mut)]
    temp_token_record: UncheckedAccount<'info>,

    /// CHECK: checked by address and in CPI
    #[account(address = mpl_token_metadata::ID)]
    token_metadata_program: UncheckedAccount<'info>,
    /// CHECK: checked in CPI
    edition: UncheckedAccount<'info>,
    /// CHECK: checked in CPI
    authorization_rules_program: UncheckedAccount<'info>,
    /// CHECK: checked in CPI
    authorization_rules: UncheckedAccount<'info>,
    /// CHECK: checked in CPI
    #[account(mut)]
    owner_token_record: UncheckedAccount<'info>,
    /// CHECK: checked in CPI
    #[account(mut)]
    destination_token_record: UncheckedAccount<'info>,
    /// CHECK: address is checked
    #[account(address = sysvar::instructions::id())]
    instructions: UncheckedAccount<'info>,

    associated_token_program: Program<'info, AssociatedToken>,
    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,
}

pub fn handle_mip1_cancel_sell<'info>(
    ctx: Context<'_, '_, '_, 'info, MIP1CancelSell<'info>>,
) -> Result<()> {
    let seller_trade_state = &ctx.accounts.seller_trade_state;
    let wallet = &ctx.accounts.wallet;
    let token_account = &ctx.accounts.token_account;
    let program_as_signer = &ctx.accounts.program_as_signer;
    let token_ata = &ctx.accounts.token_ata;
    let token_account_temp = &ctx.accounts.token_account_temp;
    let token_mint = ctx.accounts.token_mint.as_ref() as &AccountInfo;
    let metadata = &ctx.accounts.metadata;
    let edition = &ctx.accounts.edition;
    let token_program = &ctx.accounts.token_program;
    let associated_token_program = &ctx.accounts.associated_token_program;
    let instructions = &ctx.accounts.instructions;
    let system_program = &ctx.accounts.system_program;
    let authorization_rules_program = &ctx.accounts.authorization_rules_program;
    let authorization_rules = &ctx.accounts.authorization_rules;
    let owner_token_record = &ctx.accounts.owner_token_record;
    let destination_token_record = &ctx.accounts.destination_token_record;
    let temp_token_record = &ctx.accounts.temp_token_record;

    let sell_args = SellArgs::from_account_info(seller_trade_state)?;
    sell_args.check_args(
        &sell_args.seller_referral,
        &sell_args.buyer_price,
        token_mint.key,
        &token_ata.amount,
        &sell_args.payment_mint, // don't care about payment mint here
    )?;

    check_programmable(&Metadata::safe_deserialize(&metadata.data.borrow()).unwrap())?;

    let program_as_signer_seeds = &[
        PREFIX.as_bytes(),
        SIGNER.as_bytes(),
        &[ctx.bumps.program_as_signer],
    ];
    let source_token_account = if token_ata.key().eq(token_account.key) {
        // mip0 -> mip1 migration, need to move to temp token account
        let mut payload_map = HashMap::new();
        payload_map.insert(
            "SourceSeeds".to_owned(),
            PayloadType::Seeds(SeedsVec {
                seeds: vec![PREFIX.as_bytes().to_vec(), SIGNER.as_bytes().to_vec()],
            }),
        );
        payload_map.insert(
            "DestinationSeeds".to_owned(),
            PayloadType::Seeds(SeedsVec {
                seeds: vec![PREFIX.as_bytes().to_vec(), SIGNER.as_bytes().to_vec()],
            }),
        );
        let payload = Payload { map: payload_map };
        let ins = TransferBuilder::new()
            .token(token_ata.key())
            .token_owner(token_ata.owner)
            .destination_token(token_account_temp.key())
            .destination_owner(program_as_signer.key())
            .mint(token_mint.key())
            .metadata(metadata.key())
            .edition(Some(edition.key()))
            .token_record(Some(owner_token_record.key()))
            .destination_token_record(Some(temp_token_record.key()))
            .authority(program_as_signer.key())
            .payer(wallet.key())
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

        invoke_signed(
            &ins,
            &[
                wallet.to_account_info(),
                program_as_signer.to_account_info(),
                token_ata.to_account_info(),
                token_account_temp.to_account_info(),
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
                temp_token_record.to_account_info(),
            ],
            &[program_as_signer_seeds],
        )?;

        set_authority(
            CpiContext::new(
                token_program.to_account_info(),
                SetAuthority {
                    account_or_mint: token_account.to_account_info(),
                    current_authority: program_as_signer.to_account_info(),
                },
            )
            .with_signer(&[program_as_signer_seeds]),
            AuthorityType::AccountOwner,
            Some(wallet.key()),
        )?;
        token_account_temp.to_account_info()
    } else {
        token_ata.to_account_info()
    };

    let payload = Payload {
        map: HashMap::from([(
            "SourceSeeds".to_owned(),
            PayloadType::Seeds(SeedsVec {
                seeds: vec![PREFIX.as_bytes().to_vec(), SIGNER.as_bytes().to_vec()],
            }),
        )]),
    };
    let ins = TransferBuilder::new()
        .token(source_token_account.key())
        .token_owner(program_as_signer.key())
        .destination_token(token_account.key())
        .destination_owner(wallet.key())
        .mint(token_mint.key())
        .metadata(metadata.key())
        .edition(Some(edition.key()))
        .token_record(Some(temp_token_record.key()))
        .destination_token_record(Some(destination_token_record.key()))
        .authority(program_as_signer.key())
        .payer(wallet.key())
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

    invoke_signed(
        &ins,
        &[
            program_as_signer.to_account_info(),
            token_account.to_account_info(),
            source_token_account.clone(),
            wallet.to_account_info(),
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
            temp_token_record.to_account_info(),
            destination_token_record.to_account_info(),
        ],
        &[program_as_signer_seeds],
    )?;

    if token_ata.amount == 1 {
        invoke_signed(
            &spl_token::instruction::close_account(
                token_program.key,
                &source_token_account.key(),
                &wallet.key(),
                &program_as_signer.key(),
                &[],
            )?,
            &[
                source_token_account.clone(),
                wallet.to_account_info(),
                program_as_signer.to_account_info(),
                token_program.to_account_info(),
            ],
            &[program_as_signer_seeds],
        )?;
    }

    assert_is_ata(token_account, wallet.key, token_mint.key, wallet.key)?;

    close_account_anchor(seller_trade_state, wallet)?;

    msg!(
        "mip1_cancel_sell: {{\"seller_trade_state\":\"{}\",\"token_account\":\"{}\"}}",
        seller_trade_state.key(),
        token_ata.key()
    );
    msg!(
        "{{\"price\":{},\"seller_expiry\":{}}}",
        sell_args.buyer_price,
        sell_args.expiry
    );
    Ok(())
}
