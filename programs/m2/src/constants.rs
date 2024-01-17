use solana_program::{pubkey, pubkey::Pubkey};

pub const PREFIX: &str = "m2";
pub const TREASURY: &str = "treasury";
pub const SIGNER: &str = "signer";
pub const MAX_PRICE: u64 = 8000000 * 1000000000;
pub const CANCEL_AUTHORITY: Pubkey = pubkey!("CNTuB1JiQD8Xh5SoRcEmF61yivN9F7uzdSaGnRex36wi");
pub const DEFAULT_MAKER_FEE_BP: i16 = 0;
pub const DEFAULT_TAKER_FEE_BP: u16 = 250;
pub const MAX_MAKER_FEE_BP: i16 = 500;
pub const MAX_TAKER_FEE_BP: u16 = 500;
pub const DEFAULT_BID_EXPIRY_SECONDS_AFTER_NOW: i64 = 60 * 60 * 24 * 7; // 7 days

pub const VALID_PAYMENT_MINTS: [Pubkey; 8] = if cfg!(feature = "anchor-test") {
    [
        pubkey!("BJqwwqWHcA5pXAnsAnG6mMiRqKzNcg36LG4bvcqbi3PP"),
        pubkey!("CmmV5QXtqiaGQoca58HsKM2tP9qGQcAAh7gHwzZ68MBS"),
        pubkey!("11111111111111111111111111111111"),
        pubkey!("11111111111111111111111111111111"),
        pubkey!("11111111111111111111111111111111"),
        pubkey!("11111111111111111111111111111111"),
        pubkey!("11111111111111111111111111111111"),
        pubkey!("11111111111111111111111111111111"),
    ]
} else {
    [
        pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"), // USDC
        pubkey!("mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So"),  // marinade staked SOL
        pubkey!("J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn"), // Jito staked SOL
        pubkey!("3dgCCb15HMQSA4Pn3Tfii5vRk7aRqTH95LJjxzsG2Mug"), // HXD
        pubkey!("2taiJMsZVH9UWG31DASLE3qZdpiq7BsmAVRoC8kNKgyR"), // xHXD (devnet HXD)
        pubkey!("4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU"), // devnet USDC
        pubkey!("DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263"), // Bonk
        pubkey!("11111111111111111111111111111111"),
    ]
};
