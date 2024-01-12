use std::slice::Iter;

use anchor_lang::prelude::*;
use mpl_token_metadata::accounts::Metadata;
use open_creator_protocol::state::Policy;
use solana_program::{
    program::{invoke, invoke_signed},
    system_instruction,
};

use super::{assert_initialized, assert_is_ata, assert_keys_equal, is_token_owner, make_ata};
use crate::errors::ErrorCode;

pub enum DestinationSpecifier<'refs, 'a> {
    Key(&'refs Pubkey),
    Ai(&'refs AccountInfo<'a>),
}

/// Transfers token, does some cleanup and checks
///
/// # Arguments
/// * `amount` - Amount of token to transfer
/// * `payer` - Payer account, will pay for rent for new token account if needed
/// * `source_authority` - Authority of the source token account, should be allowed to transfer token
/// * `close_account_rent_receiver` - Account to receive rent if source token account is closed
/// * `optional_new_authority` - If Some, will set the new authority of the source token account to this if the source token account cannot be closed
/// * `destination_owner` - Owner of the destination token account - if the destination account is not created, it needs to be DestinationSpecifier::Ai(owner_acccount_info)
/// * `mint` - Mint of the token
/// * `source_token_account` - Source token account
/// * `destination_token_account` - Destination token account
/// * `token_program` - Token program
/// * `system_program` - System program
/// * `optional_new_owner` - If Some, will allow the destination token account to be owned by this instead of the destination owner
/// * `signer_seeds` - Seeds for the source_authority if needed
pub fn transfer_token<'refs, 'a>(
    amount: &u64,
    payer: &'refs AccountInfo<'a>,
    source_authority: &'refs AccountInfo<'a>,
    close_account_rent_receiver: &'refs AccountInfo<'a>,
    optional_new_authority: Option<&'refs AccountInfo<'a>>,
    destination_owner: DestinationSpecifier<'refs, 'a>,
    mint: &'refs AccountInfo<'a>,
    source_token_account: &'refs AccountInfo<'a>,
    destination_token_account: &'refs AccountInfo<'a>,
    token_program: &'refs AccountInfo<'a>,
    system_program: &'refs AccountInfo<'a>,
    optional_new_owner: Option<&Pubkey>,
    signer_seeds: &[&[&[u8]]],
) -> Result<spl_token::state::Account> {
    let dest_owner_key = match destination_owner {
        DestinationSpecifier::Key(key) => key,
        DestinationSpecifier::Ai(ai) => ai.key,
    };
    // initialize destination token account if needed
    if destination_token_account.data_is_empty() {
        if let DestinationSpecifier::Ai(owner_ai) = destination_owner {
            // we can only create an ATA if we have the owner's account info
            make_ata(
                destination_token_account.to_account_info(),
                payer.to_account_info(),
                owner_ai.to_account_info(),
                mint.to_account_info(),
                token_program.to_account_info(),
                system_program.to_account_info(),
            )?;
        } else {
            // we can't create an ATA, so we need to throw error
            return Err(ErrorCode::UninitializedAccount.into());
        }
    } else {
        let is_owner = is_token_owner(destination_token_account, dest_owner_key)?;
        if !is_owner {
            return Err(ErrorCode::IncorrectOwner.into());
        }
    }

    // transfer the token
    invoke_signed(
        &spl_token::instruction::transfer(
            token_program.key,
            source_token_account.key,
            destination_token_account.key,
            source_authority.key,
            &[],
            *amount,
        )?,
        &[
            source_token_account.clone(),
            destination_token_account.clone(),
            source_authority.clone(),
        ],
        signer_seeds,
    )?;

    let source_parsed: spl_token::state::Account = assert_initialized(source_token_account)?;
    // we can clean up the source token account if we have ownership of the source
    if source_parsed.owner == *source_authority.key {
        if source_parsed.amount == 0 {
            // close the account if it's empty
            invoke_signed(
                &spl_token::instruction::close_account(
                    token_program.key,
                    source_token_account.key,
                    close_account_rent_receiver.key,
                    source_authority.key,
                    &[],
                )?,
                &[
                    source_token_account.clone(),
                    close_account_rent_receiver.clone(),
                    source_authority.clone(),
                ],
                signer_seeds,
            )?;
        } else if let Some(new_authority) = optional_new_authority {
            // set the new authority if we have one
            invoke_signed(
                &spl_token::instruction::set_authority(
                    token_program.key,
                    source_token_account.key,
                    Some(new_authority.key),
                    spl_token::instruction::AuthorityType::AccountOwner,
                    source_authority.key,
                    &[],
                )?,
                &[
                    source_token_account.clone(),
                    source_authority.clone(),
                    new_authority.clone(),
                ],
                signer_seeds,
            )?;
        }
    }

    assert_is_ata(
        destination_token_account,
        dest_owner_key,
        mint.key,
        if let Some(new_owner) = optional_new_owner {
            new_owner
        } else {
            dest_owner_key
        },
    )
}

pub struct TransferListingPaymentSplArgs<'r, 'info> {
    pub payer: &'r AccountInfo<'info>,
    pub buyer: &'r AccountInfo<'info>,
    pub mint: &'r AccountInfo<'info>,

    pub payment_source_token_account: &'r AccountInfo<'info>,
    pub payment_seller_token_account: &'r AccountInfo<'info>,
    pub payment_treasury_token_account: &'r AccountInfo<'info>,

    pub system_program: &'r AccountInfo<'info>,
    pub token_program: &'r AccountInfo<'info>,
}

pub fn transfer_listing_payment<'info>(
    buyer_price: u64,
    actual_maker_fee_bp: i16,
    actual_taker_fee_bp: u16,
    taker: &AccountInfo<'info>,
    seller: &AccountInfo<'info>,
    escrow_payment_account: &AccountInfo<'info>,
    auction_house_treasury: &AccountInfo<'info>,
    listing_spl_args: Option<TransferListingPaymentSplArgs<'_, 'info>>,
    signer_seeds: &[&[&[u8]]],
) -> Result<(i64, u64)> {
    // payer pays maker/taker fees
    // seller is payer and taker
    //   seller as payer pays (maker_fee + taker_fee) to treasury
    //   buyer as maker needs to pay args.price + maker_fee + royalty
    //   seller gets (args.price + maker_fee) from buyer
    // buyer is payer and taker
    //   buyer as payer pays (maker_fee + taker_fee) to treasury
    //   buyer as taker needs to pay (args.price + taker_fee + royalty)
    //   seller gets (args.price - maker_fee) from buyer
    // royalty is also paid ON TOP of the price

    let maker_fee = (buyer_price as i128)
        .checked_mul(actual_maker_fee_bp as i128)
        .ok_or(ErrorCode::NumericalOverflow)?
        .checked_div(10000)
        .ok_or(ErrorCode::NumericalOverflow)? as i64;
    let taker_fee = (buyer_price as u128)
        .checked_mul(actual_taker_fee_bp as u128)
        .ok_or(ErrorCode::NumericalOverflow)?
        .checked_div(10000)
        .ok_or(ErrorCode::NumericalOverflow)? as u64;
    let seller_will_get_from_buyer = if taker.key.eq(seller.key) {
        (buyer_price as i64)
            .checked_add(maker_fee)
            .ok_or(ErrorCode::NumericalOverflow)?
    } else {
        (buyer_price as i64)
            .checked_sub(maker_fee)
            .ok_or(ErrorCode::NumericalOverflow)?
    } as u64;
    let total_platform_fee = (maker_fee
        .checked_add(taker_fee as i64)
        .ok_or(ErrorCode::NumericalOverflow)?) as u64;

    if let Some(listing_spl_args) = &listing_spl_args {
        // transfer SPL token

        transfer_token(
            &seller_will_get_from_buyer,
            listing_spl_args.payer,
            escrow_payment_account,
            listing_spl_args.buyer,
            None,
            DestinationSpecifier::Ai(seller),
            listing_spl_args.mint,
            listing_spl_args.payment_source_token_account,
            listing_spl_args.payment_seller_token_account,
            listing_spl_args.token_program,
            listing_spl_args.system_program,
            None,
            signer_seeds,
        )?;

        if total_platform_fee > 0 {
            if taker.key == seller.key {
                transfer_token(
                    &total_platform_fee,
                    listing_spl_args.payer,
                    taker,
                    taker,
                    None,
                    DestinationSpecifier::Ai(auction_house_treasury),
                    listing_spl_args.mint,
                    listing_spl_args.payment_seller_token_account,
                    listing_spl_args.payment_treasury_token_account,
                    listing_spl_args.token_program,
                    listing_spl_args.system_program,
                    None,
                    &[],
                )?;
            } else {
                transfer_token(
                    &total_platform_fee,
                    listing_spl_args.payer,
                    escrow_payment_account,
                    listing_spl_args.buyer,
                    None,
                    DestinationSpecifier::Ai(auction_house_treasury),
                    listing_spl_args.mint,
                    listing_spl_args.payment_source_token_account,
                    listing_spl_args.payment_treasury_token_account,
                    listing_spl_args.token_program,
                    listing_spl_args.system_program,
                    None,
                    signer_seeds,
                )?;
            }
        }
    } else {
        // transfer native SOL
        invoke_signed(
            &system_instruction::transfer(
                escrow_payment_account.key,
                seller.key,
                seller_will_get_from_buyer,
            ),
            &[
                escrow_payment_account.to_account_info(),
                seller.to_account_info(),
            ],
            signer_seeds,
        )?;

        if total_platform_fee > 0 {
            if taker.key == seller.key {
                invoke(
                    &system_instruction::transfer(
                        taker.key,
                        auction_house_treasury.key,
                        total_platform_fee,
                    ),
                    &[
                        taker.to_account_info(),
                        auction_house_treasury.to_account_info(),
                    ],
                )?;
            } else {
                invoke_signed(
                    &system_instruction::transfer(
                        escrow_payment_account.key,
                        auction_house_treasury.key,
                        total_platform_fee,
                    ),
                    &[
                        escrow_payment_account.to_account_info(),
                        auction_house_treasury.to_account_info(),
                    ],
                    signer_seeds,
                )?;
            }
        }
    }

    Ok((maker_fee, taker_fee))
}

pub struct TransferCreatorSplArgs<'r, 'info> {
    pub buyer: &'r AccountInfo<'info>,
    pub payer: &'r AccountInfo<'info>,
    pub mint: &'r AccountInfo<'info>,

    pub payment_source_token_account: &'r AccountInfo<'info>,
    pub system_program: &'r AccountInfo<'info>,
    pub token_program: &'r AccountInfo<'info>,
}

#[allow(clippy::too_many_arguments)]
pub fn pay_creator_fees<'r, 'a>(
    creator_accounts: &mut Iter<'r, AccountInfo<'a>>,
    policy: Option<&Account<'a, Policy>>,
    metadata: &'r Metadata,
    escrow_payment_account: &AccountInfo<'a>,
    signer_seeds: &[&[&[u8]]],
    total_price: u64,
    buyer_creator_royalty_bp: u16,
    creator_spl_args: Option<TransferCreatorSplArgs<'_, 'a>>,
) -> Result<u64> {
    let creators = if let Some(creators) = &metadata.creators {
        creators
    } else {
        return Ok(0);
    };

    if creators.is_empty() {
        return Ok(0);
    }

    let royalty_bp = match policy {
        None => metadata.seller_fee_basis_points,
        Some(p) => match &p.dynamic_royalty {
            None => metadata.seller_fee_basis_points,
            Some(dynamic_royalty) => {
                dynamic_royalty.get_royalty_bp(total_price, metadata.seller_fee_basis_points)
            }
        },
    };

    let total_fee = (royalty_bp as u128)
        .checked_mul(total_price as u128)
        .ok_or(ErrorCode::NumericalOverflow)?
        .checked_div(10000)
        .ok_or(ErrorCode::NumericalOverflow)?
        .checked_mul(buyer_creator_royalty_bp as u128)
        .ok_or(ErrorCode::NumericalOverflow)?
        .checked_div(10000)
        .ok_or(ErrorCode::NumericalOverflow)? as u64;
    if total_fee == 0 {
        return Ok(0);
    }
    let mut total_fee_paid = 0u64;
    for creator in creators {
        let pct = creator.share as u128;
        let creator_fee = pct
            .checked_mul(total_fee as u128)
            .ok_or(ErrorCode::NumericalOverflow)?
            .checked_div(100)
            .ok_or(ErrorCode::NumericalOverflow)? as u64;
        let current_creator_info = next_account_info(creator_accounts)?;
        if let Some(spl_args) = &creator_spl_args {
            // transfer SPL token, current_creator_info should be the creator's ATA
            if creator_fee == 0 {
                continue;
            }

            let dest_specifier = if current_creator_info.data_is_empty() {
                // creator's account info is required if the creator's ATA is not initialized, we expect clients to structure remaining accounts correctly
                let next_ai = next_account_info(creator_accounts)?;
                assert_keys_equal(&creator.address, next_ai.key)?;
                DestinationSpecifier::Ai(next_ai)
            } else {
                // since creator ATA is initialized, we can pass in a fake accountInfo with only the pubkey valid
                DestinationSpecifier::Key(&creator.address)
            };
            transfer_token(
                &creator_fee,
                spl_args.payer,
                escrow_payment_account,
                spl_args.buyer,
                None,
                dest_specifier,
                spl_args.mint,
                spl_args.payment_source_token_account,
                current_creator_info,
                spl_args.token_program,
                spl_args.system_program,
                None,
                signer_seeds,
            )?;
        } else {
            assert_keys_equal(&creator.address, current_creator_info.key)?;
            if creator_fee + current_creator_info.lamports() >= Rent::get()?.minimum_balance(0) {
                invoke_signed(
                    &system_instruction::transfer(
                        escrow_payment_account.key,
                        current_creator_info.key,
                        creator_fee,
                    ),
                    &[escrow_payment_account.clone(), current_creator_info.clone()],
                    signer_seeds,
                )?;
                total_fee_paid = total_fee_paid
                    .checked_add(creator_fee)
                    .ok_or(ErrorCode::NumericalOverflow)?;
            }
        }
    }

    Ok(total_fee_paid)
}
