#![allow(clippy::result_large_err)]

pub mod constants;
mod errors;
mod m2_ins;
pub mod mip1_ins;
mod ocp_ins;
pub mod states;
mod utils;

use crate::m2_ins::*;
use crate::mip1_ins::*;
use crate::ocp_ins::*;
use anchor_lang::prelude::*;

anchor_lang::declare_id!("M2mx93ekt1fmXSVkTrUL9xVFHkmME8HTUi5Cyc5aF7K");

#[program]
pub mod m2 {
    use super::*;

    pub fn withdraw_from_treasury<'info>(
        ctx: Context<'_, '_, '_, 'info, WithdrawFromTreasury<'info>>,
        amount: u64,
    ) -> Result<()> {
        m2_ins::withdraw_from_treasury::handle(ctx, amount)
    }

    pub fn update_auction_house<'info>(
        ctx: Context<'_, '_, '_, 'info, UpdateAuctionHouse<'info>>,
        seller_fee_basis_points: Option<u16>,
        buyer_referral_bp: Option<u16>,
        seller_referral_bp: Option<u16>,
        requires_notary: Option<bool>,
        nprob: Option<u8>,
    ) -> Result<()> {
        m2_ins::update_auction_house::handle(
            ctx,
            seller_fee_basis_points,
            buyer_referral_bp,
            seller_referral_bp,
            requires_notary,
            nprob,
        )
    }

    pub fn withdraw<'info>(
        ctx: Context<'_, '_, '_, 'info, Withdraw<'info>>,
        escrow_payment_bump: u8,
        amount: u64,
    ) -> Result<()> {
        m2_ins::withdraw::handle(ctx, escrow_payment_bump, amount)
    }

    pub fn deposit<'info>(
        ctx: Context<'_, '_, '_, 'info, Deposit<'info>>,
        _escrow_payment_bump: u8,
        amount: u64,
    ) -> Result<()> {
        m2_ins::deposit::handle(ctx, amount)
    }

    pub fn sell<'info>(
        ctx: Context<'_, '_, '_, 'info, Sell<'info>>,
        _seller_state_bump: u8,
        program_as_signer_bump: u8,
        buyer_price: u64,
        token_size: u64,
        seller_state_expiry: i64,
    ) -> Result<()> {
        m2_ins::sell::handle(
            ctx,
            program_as_signer_bump,
            buyer_price,
            token_size,
            seller_state_expiry,
        )
    }

    pub fn cancel_sell<'info>(
        ctx: Context<'_, '_, '_, 'info, CancelSell<'info>>,
        buyer_price: u64,
        token_size: u64,
        seller_state_expiry: i64,
    ) -> Result<()> {
        m2_ins::cancel_sell::handle(ctx, buyer_price, token_size, seller_state_expiry)
    }

    pub fn buy<'info>(
        ctx: Context<'_, '_, '_, 'info, Buy<'info>>,
        _buyer_state_bump: u8,
        escrow_payment_bump: u8,
        buyer_price: u64,
        token_size: u64,
        buyer_state_expiry: i64,
    ) -> Result<()> {
        m2_ins::buy::handle(
            ctx,
            escrow_payment_bump,
            buyer_price,
            token_size,
            buyer_state_expiry,
        )
    }

    pub fn buy_v2<'info>(
        ctx: Context<'_, '_, '_, 'info, BuyV2<'info>>,
        buyer_price: u64,
        token_size: u64,
        buyer_state_expiry: i64,
        buyer_creator_royalty_bp: u16,
        extra_args: Vec<u8>,
    ) -> Result<()> {
        m2_ins::buy_v2::handle(
            ctx,
            buyer_price,
            token_size,
            buyer_state_expiry,
            buyer_creator_royalty_bp,
            &extra_args,
        )
    }

    pub fn cancel_buy<'info>(
        ctx: Context<'_, '_, '_, 'info, CancelBuy<'info>>,
        buyer_price: u64,
        token_size: u64,
        buyer_state_expiry: i64,
    ) -> Result<()> {
        m2_ins::cancel_buy::handle(ctx, buyer_price, token_size, buyer_state_expiry)
    }

    pub fn ocp_sell<'info>(
        ctx: Context<'_, '_, '_, 'info, OCPSell<'info>>,
        args: OCPSellArgs,
    ) -> Result<()> {
        ocp_ins::ocp_sell::handle(ctx, args)
    }

    pub fn ocp_cancel_sell<'info>(
        ctx: Context<'_, '_, '_, 'info, OCPCancelSell<'info>>,
    ) -> Result<()> {
        ocp_ins::ocp_cancel_sell::handle(ctx)
    }

    pub fn ocp_execute_sale_v2<'info>(
        ctx: Context<'_, '_, '_, 'info, OCPExecuteSaleV2<'info>>,
        args: OCPExecuteSaleV2Args,
    ) -> Result<()> {
        ocp_ins::ocp_execute_sale_v2::handle(ctx, args)
    }

    pub fn execute_sale_v2<'info>(
        ctx: Context<'_, '_, '_, 'info, ExecuteSaleV2<'info>>,
        escrow_payment_bump: u8,
        program_as_signer_bump: u8,
        buyer_price: u64,
        token_size: u64,
        _buyer_state_expiry: i64,
        _seller_state_expiry: i64,
        maker_fee_bp: i16,
        taker_fee_bp: u16,
    ) -> Result<()> {
        m2_ins::execute_sale_v2::handle(
            ctx,
            escrow_payment_bump,
            program_as_signer_bump,
            buyer_price,
            token_size,
            maker_fee_bp,
            taker_fee_bp,
        )
    }

    pub fn mip1_sell<'info>(
        ctx: Context<'_, '_, '_, 'info, MIP1Sell<'info>>,
        args: MIP1SellArgs,
    ) -> Result<()> {
        mip1_ins::mip1_sell::handle_mip1_sell(ctx, &args)
    }

    pub fn mip1_execute_sale_v2<'info>(
        ctx: Context<'_, '_, '_, 'info, MIP1ExecuteSaleV2<'info>>,
        args: MIP1ExecuteSaleV2Args,
    ) -> Result<()> {
        mip1_ins::mip1_execute_sale_v2::handle_mip1_execute_sale(ctx, args)
    }

    pub fn mip1_cancel_sell<'info>(
        ctx: Context<'_, '_, '_, 'info, MIP1CancelSell<'info>>,
    ) -> Result<()> {
        mip1_ins::mip1_cancel_sell::handle_mip1_cancel_sell(ctx)
    }
}
