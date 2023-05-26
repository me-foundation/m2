use {crate::constants::*, crate::errors::ErrorCode, crate::states::*, anchor_lang::prelude::*};

#[derive(Accounts)]
pub struct UpdateAuctionHouse<'info> {
    payer: Signer<'info>,
    /// CHECK: notary is not dangerous because we don't read or write from this account
    notary: UncheckedAccount<'info>,
    authority: Signer<'info>,
    /// CHECK: new_authority
    new_authority: UncheckedAccount<'info>,
    /// CHECK: treasury_withdrawal_destination
    #[account(mut)]
    treasury_withdrawal_destination: UncheckedAccount<'info>,
    #[account(mut, seeds=[PREFIX.as_bytes(), auction_house.creator.as_ref()], bump=auction_house.bump, has_one=authority)]
    auction_house: Account<'info, AuctionHouse>,
    system_program: Program<'info, System>,
}

pub fn handle<'info>(
    ctx: Context<'_, '_, '_, 'info, UpdateAuctionHouse<'info>>,
    seller_fee_basis_points: Option<u16>,
    buyer_referral_bp: Option<u16>,
    seller_referral_bp: Option<u16>,
    requires_notary: Option<bool>,
    nprob: Option<u8>,
) -> Result<()> {
    let new_authority = &ctx.accounts.new_authority;
    let auction_house = &mut ctx.accounts.auction_house;
    let treasury_withdrawal_destination = &ctx.accounts.treasury_withdrawal_destination;

    if let Some(sfbp) = seller_fee_basis_points {
        if sfbp > 10000 {
            return Err(ErrorCode::InvalidBasisPoints.into());
        }

        auction_house.seller_fee_basis_points = sfbp;
    }

    if let Some(require_notary) = requires_notary {
        auction_house.requires_notary = require_notary;
        auction_house.notary = ctx.accounts.notary.key();
    }

    if let Some(bbp) = buyer_referral_bp {
        auction_house.buyer_referral_bp = bbp;
    }
    if let Some(sbp) = seller_referral_bp {
        auction_house.seller_referral_bp = sbp;
    }
    if let Some(_nprob) = nprob {
        auction_house.nprob = _nprob;
    }

    let referral_bp = auction_house
        .buyer_referral_bp
        .checked_add(auction_house.seller_referral_bp)
        .ok_or(ErrorCode::NumericalOverflow)?;
    if referral_bp > auction_house.seller_fee_basis_points {
        return Err(ErrorCode::InvalidBasisPoints.into());
    }

    auction_house.authority = new_authority.key();
    auction_house.treasury_withdrawal_destination = treasury_withdrawal_destination.key();
    Ok(())
}
