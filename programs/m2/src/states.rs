use anchor_lang::{prelude::*, AnchorDeserialize, Discriminator};

use crate::errors::ErrorCode;

#[account]
#[derive(Default, Copy)]
pub struct BuyerTradeState {
    // Byte offsets:
    // 0
    // Discriminator

    // 8
    pub auction_house_key: Pubkey,
    // 40
    pub buyer: Pubkey,
    // 72
    pub buyer_referral: Pubkey,
    // 104
    pub buyer_price: u64,
    // 112
    pub token_mint: Pubkey,
    // 144
    pub token_size: u64,
    // 152
    pub bump: u8,
    // 153
    pub expiry: i64, // in unix timestamp in seconds
}

impl BuyerTradeState {
    pub const LEN: usize = 161; // including the 8 bytes discriminator
}

#[account]
#[derive(Default, Copy)]
pub struct SellerTradeState {
    // Byte offsets:
    // 0
    // Discriminator

    // 8
    pub auction_house_key: Pubkey,
    // 40
    pub seller: Pubkey,
    // 72
    pub seller_referral: Pubkey,
    // 104
    pub buyer_price: u64,
    // 112
    pub token_mint: Pubkey,
    // 144
    pub token_account: Pubkey,
    // 176
    pub token_size: u64,
    // 184
    pub bump: u8,
    // 185
    pub expiry: i64, // in unix timestamp in seconds
}

impl SellerTradeState {
    pub const LEN: usize = 193; // including the 8 bytes discriminator
}

#[allow(dead_code)]
pub const AUCTION_HOUSE_SIZE: usize = 8 + // key
32 + // auction_house_treasury
32 + // treasury_withdrawal_destination
32 + // authority
32 + // creator
32 + // notary
1 +  // bump
1 +  // treasury_bump
2 +  // seller_fee_basis_points
2 +  // buyer_referral_bp
2 +  // seller_referral_bp
1 +  // requires_notary
1 +  // nprob, notary enforce probability, 0-100
219; // padding

#[account]
pub struct AuctionHouse {
    pub auction_house_treasury: Pubkey,
    pub treasury_withdrawal_destination: Pubkey,
    pub authority: Pubkey,
    pub creator: Pubkey,
    pub notary: Pubkey,
    pub bump: u8,
    pub treasury_bump: u8,
    pub seller_fee_basis_points: u16,
    pub buyer_referral_bp: u16,
    pub seller_referral_bp: u16,
    pub requires_notary: bool,
    pub nprob: u8, // notary enforce probability
}

#[account]
#[derive(Default, Copy)]
pub struct BuyerTradeStateV2 {
    pub auction_house_key: Pubkey,
    pub buyer: Pubkey,
    pub buyer_referral: Pubkey,
    pub buyer_price: u64,
    pub token_mint: Pubkey,
    pub token_size: u64,
    pub bump: u8,
    pub expiry: i64,
    pub buyer_creator_royalty_bp: u16,
}

impl BuyerTradeStateV2 {
    pub const LEN: usize = 8 + // discriminator
    32 + // auction_house_key
    32 + // buyer
    32 + // buyer_referral
    8 + // buyer_price
    32 + // token_mint
    8 + // token_size
    1 + // bump
    8 + // expiry
    2 + // buyer_creator_ryoalty_bp
    157; // padding to 320 bytes

    pub fn from_bid_args(args: &BidArgs) -> Self {
        BuyerTradeStateV2 {
            auction_house_key: args.auction_house_key,
            buyer: args.buyer,
            buyer_referral: args.buyer_referral,
            buyer_price: args.buyer_price,
            token_mint: args.token_mint,
            token_size: args.token_size,
            bump: args.bump,
            expiry: args.expiry,
            buyer_creator_royalty_bp: args.buyer_creator_royalty_bp,
        }
    }
}

pub struct BidArgs {
    pub auction_house_key: Pubkey,
    pub buyer: Pubkey,
    pub buyer_referral: Pubkey,
    pub buyer_price: u64,
    pub token_mint: Pubkey,
    pub token_size: u64,
    pub bump: u8,
    pub expiry: i64, // in unix timestamp in seconds
    pub buyer_creator_royalty_bp: u16,
}

impl BidArgs {
    pub fn check_args(
        &self,
        buyer_referral: &Pubkey,
        buyer_price: u64,
        token_mint: &Pubkey,
        token_size: u64,
    ) -> Result<()> {
        if self.buyer_referral != *buyer_referral
            || self.buyer_price != buyer_price
            || self.token_mint != *token_mint
            || self.token_size != token_size
        {
            Err(ErrorCode::InvalidAccountState.into())
        } else {
            Ok(())
        }
    }

    pub fn from_account_info(info: &AccountInfo) -> Result<Self> {
        let mut account_data: &[u8] = &info.try_borrow_data()?;
        let discrimantor = &account_data[0..8];
        if discrimantor == BuyerTradeState::discriminator() {
            let bts = BuyerTradeState::try_deserialize(&mut account_data)?;
            Ok(BidArgs {
                auction_house_key: bts.auction_house_key,
                buyer: bts.buyer,
                buyer_referral: bts.buyer_referral,
                buyer_price: bts.buyer_price,
                token_mint: bts.token_mint,
                token_size: bts.token_size,
                bump: bts.bump,
                expiry: bts.expiry,
                buyer_creator_royalty_bp: 0,
            })
        } else if discrimantor == BuyerTradeStateV2::discriminator() {
            let bts = BuyerTradeStateV2::try_deserialize(&mut account_data)?;
            Ok(BidArgs {
                auction_house_key: bts.auction_house_key,
                buyer: bts.buyer,
                buyer_referral: bts.buyer_referral,
                buyer_price: bts.buyer_price,
                token_mint: bts.token_mint,
                token_size: bts.token_size,
                bump: bts.bump,
                expiry: bts.expiry,
                buyer_creator_royalty_bp: bts.buyer_creator_royalty_bp,
            })
        } else {
            Err(ErrorCode::InvalidDiscriminator.into())
        }
    }
}
