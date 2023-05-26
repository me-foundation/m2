<img src="./docs/logo.png" width="400">

# M2

Marketplace Smart Contract for M2mx93ekt1fmXSVkTrUL9xVFHkmME8HTUi5Cyc5aF7K

# Setup

```sh
$ anchor build
$ anchor test
```

# IDL
- [IDL - m2.json](src/idl/m2.json)
- [Types - m2.ts](src/types/m2.ts)

Recommend to use the IDL to directly parse or CPI into the M2 onchain program with your generated SDK.

# Entrypoints

| Anchor Entrypoint    | Action                             | Notes                                  |
| -------------------- | ---------------------------------- | -------------------------------------- |
| buy_v2               | Make a single bid                  | Buyer                                  |
| cancel_buy           | Cancel a single bid                | Buyer                                  |
| deposit              | Deposit into the buyer escrow PDA  | Buyer                                  |
| withdraw             | Withdraw from the buyer escrow PDA | Buyer                                  |
| sell                 | List the NFT                       | Seller                                 |
| cancel_sell          | Delist the NFT                     | Seller                                 |
| execute_sale_v2      | Execute the swap                   | Buyer or Seller                        |
| mip1_sell            | List the pNFT                      | pNFT (MIP1) version of the Entrypoints |
| mip1_cancel_sell     | Delist the pNFT                    | pNFT (MIP1) version of the Entrypoints |
| mip1_execute_sale_v2 | Execute the swap for pNFT          | pNFT (MIP1) version of the Entrypoints |
| ocp_sell             | List the OCP NFT                   | OCP version of the Entrypoints         |
| ocp_cancel_sell      | Delist the OCP NFT                 | OCP version of the Entrypoints         |
| ocp_execute_sale_v2  | Execute the swap for OCP NFT       | OCP version of the Entrypoints         |

----

| Transaction Example      | Entrypoints Combination         |
| ------------------------ | ------------------------------- |
| Buy Now (as buyer)       | deposit + buy + execute_sale_v2 |
| Change Price (as seller) | sell                            |
| Accept Offer (as seller) | sell + execute_sale_v2          |

----

| PDA Account                          | Seeds                                                                                                                                |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| escrow_payment_account   (For Buyer) | `[PREFIX.as_bytes(), auction_house.key().as_ref(), buyer.key().as_ref()]`                                                            |
| auction_house_treasury               | `[PREFIX.as_bytes(), auction_house.key().as_ref(), TREASURY.as_bytes()]`                                                             |
| buyer_trade_state                    | `[PREFIX.as_bytes(), buyer.key().as_ref(), auction_house.key().as_ref(), token_mint.key().as_ref()]`                                |
| seller_trade_state                   | `[PREFIX.as_bytes(), seller.key().as_ref(), auction_house.key().as_ref(), token_account.key().as_ref(), token_mint.key().as_ref()]` |
| program_as_signer                    | `[PREFIX.as_bytes(), SIGNER.as_bytes()]`                                                                                             |

```
pub const PREFIX: &str = "m2";
pub const TREASURY: &str = "treasury";
pub const SIGNER: &str = "signer";
```

# License
Apache 2.0

# Implementation Reference
Auction House v1.0.0 (Apache 2.0)
https://github.com/metaplex-foundation/metaplex/blob/v1.0.0/LICENSE
