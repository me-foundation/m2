use anchor_lang::prelude::*;

#[error_code]
pub enum ErrorCode {
    // 300
    #[msg("PublicKeyMismatch")]
    PublicKeyMismatch,
    // 301
    #[msg("InvalidMintAuthority")]
    InvalidMintAuthority,
    // 302
    #[msg("UninitializedAccount")]
    UninitializedAccount,
    // 303
    #[msg("IncorrectOwner")]
    IncorrectOwner,
    // 304
    #[msg("PublicKeysShouldBeUnique")]
    PublicKeysShouldBeUnique,
    // 305
    #[msg("StatementFalse")]
    StatementFalse,
    // 306
    #[msg("NotRentExempt")]
    NotRentExempt,
    // 307
    #[msg("NumericalOverflow")]
    NumericalOverflow,
    // 308
    #[msg("Expected a sol account but got an spl token account instead")]
    ExpectedSolAccount,
    // 309
    #[msg("Cannot exchange sol for sol")]
    CannotExchangeSOLForSol,
    // 310
    #[msg("If paying with sol, sol wallet must be signer")]
    SOLWalletMustSign,
    // 311
    #[msg("Cannot take this action without auction house signing too")]
    CannotTakeThisActionWithoutAuctionHouseSignOff,
    // 312
    #[msg("No payer present on this txn")]
    NoPayerPresent,
    // 313
    #[msg("Derived key invalid")]
    DerivedKeyInvalid,
    // 314
    #[msg("Metadata doesn't exist")]
    MetadataDoesntExist,
    // 315
    #[msg("Invalid token amount")]
    InvalidTokenAmount,
    // 316
    #[msg("Both parties need to agree to this sale")]
    BothPartiesNeedToAgreeToSale,
    // 317
    #[msg("Cannot match free sales unless the auction house or seller signs off")]
    CannotMatchFreeSalesWithoutAuctionHouseOrSellerSignoff,
    // 318
    #[msg("This sale requires a signer")]
    SaleRequiresSigner,
    // 319
    #[msg("Old seller not initialized")]
    OldSellerNotInitialized,
    // 320
    #[msg("Seller ata cannot have a delegate set")]
    SellerATACannotHaveDelegate,
    // 321
    #[msg("Buyer ata cannot have a delegate set")]
    BuyerATACannotHaveDelegate,
    // 322
    #[msg("No valid signer present")]
    NoValidSignerPresent,
    // 323
    #[msg("Invalid BP")]
    InvalidBasisPoints,
    // 324
    #[msg("Invalid notary")]
    InvalidNotary,
    // 325
    #[msg("Empty trade state")]
    EmptyTradeState,
    // 326
    #[msg("Invalid expiry")]
    InvalidExpiry,
    // 327
    #[msg("Invalid price")]
    InvalidPrice,
    // 328
    #[msg("Invalid remainning accounts without program_as_signer")]
    InvalidRemainingAccountsWithoutProgramAsSigner,
    // 329
    #[msg("Invalid bump")]
    InvalidBump,
    // 330
    #[msg("Invalid create auction house nonce")]
    InvalidCreateAuctionHouseNonce,
    // 331
    #[msg("Invalid account state")]
    InvalidAccountState,
    // 332
    #[msg("Invalid discriminator")]
    InvalidDiscriminator,
    // 333
    #[msg("Invalid platform fee bp")]
    InvalidPlatformFeeBp,
    // 334
    #[msg("Invalid token mint")]
    InvalidTokenMint,
    // 335
    #[msg("Invalid token standard")]
    InvalidTokenStandard,
    // 336
    #[msg("Deprecated")]
    Deprecated,
    #[msg("Missing remaining account")]
    MissingRemainingAccount,
}
