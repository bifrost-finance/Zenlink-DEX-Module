// Copyright 2020-2021 Zenlink
// Licensed under GPL-3.0.

//! # SWAP Module
//!
//! ## Overview
//!
//! Built-in decentralized exchange modules in Substrate network, the swap
//! mechanism refers to the design of Uniswap V2.

use super::*;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

impl<T: Config> Pallet<T> {
    /// The account ID of a pair account
    pub fn pair_account_id(asset_0: AssetId, asset_1: AssetId) -> T::AccountId {
        let (asset_0, asset_1) = Self::sort_asset_id(asset_0, asset_1);
        let pair_hash: T::Hash = T::Hashing::hash_of(&(asset_0, asset_1));

        T::PalletId::get().into_sub_account(pair_hash.as_ref())
    }

    /// Sorted the foreign id of assets pair
    pub fn sort_asset_id(asset_0: AssetId, asset_1: AssetId) -> (AssetId, AssetId) {
        if asset_0 < asset_1 {
            (asset_0, asset_1)
        } else {
            (asset_1, asset_0)
        }
    }

    /// The account ID of a pair account from storage
    pub fn get_pair_account_id(asset_0: AssetId, asset_1: AssetId) -> Option<T::AccountId> {
        let (asset_0, asset_1) = Self::sort_asset_id(asset_0, asset_1);

        Self::lp_metadata((asset_0, asset_1)).map(|(pair_account, _)| pair_account)
    }

    pub fn mutate_lp_pairs(asset_0: AssetId, asset_1: AssetId) {
        LiquidityPairs::<T>::mutate(|pairs| {
            pairs.push(Self::sort_asset_id(asset_0, asset_1));
        })
    }

    pub fn get_lp_pair(index: u32) -> Option<(AssetId, AssetId)> {
        let pairs = Self::lp_pairs();
        let index = index as usize;
        if index >= pairs.len() {
            None
        } else {
            Some(pairs[index])
        }
    }

    /// Sorted the foreign id of assets pair
    pub fn sort_asset_id(asset_0: AssetId, asset_1: AssetId) -> (AssetId, AssetId) {
        if asset_0 < asset_1 {
            (asset_0, asset_1)
        } else {
            (asset_1, asset_0)
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn inner_add_liquidity(
        who: &T::AccountId,
        asset_0: AssetId,
        asset_1: AssetId,
        amount_0_desired: AssetBalance,
        amount_1_desired: AssetBalance,
        amount_0_min: AssetBalance,
        amount_1_min: AssetBalance,
    ) -> DispatchResult {
        LiquidityMeta::<T>::try_mutate(Self::sort_asset_id(asset_0, asset_1), |meta| {
            if let Some((pair_account, total_liquidity)) = meta {
                let reserve_0 = T::MultiAssetsHandler::balance_of(asset_0, pair_account);
                let reserve_1 = T::MultiAssetsHandler::balance_of(asset_1, pair_account);

                let (amount_0, amount_1) = Self::calculate_added_amount(
                    amount_0_desired,
                    amount_1_desired,
                    amount_0_min,
                    amount_1_min,
                    reserve_0,
                    reserve_1,
                )?;

                let balance_asset_0 = T::MultiAssetsHandler::balance_of(asset_0, who);
                let balance_asset_1 = T::MultiAssetsHandler::balance_of(asset_1, who);
                ensure!(
                    balance_asset_0 >= amount_0 && balance_asset_1 >= amount_1,
                    Error::<T>::InsufficientAssetBalance
                );

                let mint_liquidity = Self::calculate_liquidity(
                    amount_0,
                    amount_1,
                    reserve_0,
                    reserve_1,
                    *total_liquidity,
                );
                ensure!(mint_liquidity > Zero::zero(), Error::<T>::Overflow);

                *total_liquidity =
                    total_liquidity.checked_add(mint_liquidity).ok_or(Error::<T>::Overflow)?;
                Self::mutate_liquidity(asset_0, asset_1, who, mint_liquidity, true)?;

                T::MultiAssetsHandler::transfer(asset_0, who, &pair_account, amount_0)?;
                T::MultiAssetsHandler::transfer(asset_1, who, &pair_account, amount_1)?;

                Self::deposit_event(Event::LiquidityAdded(
                    who.clone(),
                    asset_0,
                    asset_1,
                    amount_0,
                    amount_1,
                    mint_liquidity,
                ));

                Ok(())
            } else {
                Err(Error::<T>::PairNotExists.into())
            }
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn inner_remove_liquidity(
        who: &T::AccountId,
        asset_0: AssetId,
        asset_1: AssetId,
        remove_liquidity: AssetBalance,
        amount_0_min: AssetBalance,
        amount_1_min: AssetBalance,
        recipient: &T::AccountId,
    ) -> DispatchResult {
        ensure!(
            Self::lp_ledger(((asset_0, asset_1), who)) >= remove_liquidity,
            Error::<T>::InsufficientLiquidity
        );

        LiquidityMeta::<T>::try_mutate(Self::sort_asset_id(asset_0, asset_1), |meta| {
            if let Some((pair_account, total_liquidity)) = meta {
                let reserve_0 = T::MultiAssetsHandler::balance_of(asset_0, &pair_account);
                let reserve_1 = T::MultiAssetsHandler::balance_of(asset_1, &pair_account);

                let amount_0 =
                    Self::calculate_share_amount(remove_liquidity, *total_liquidity, reserve_0);
                let amount_1 =
                    Self::calculate_share_amount(remove_liquidity, *total_liquidity, reserve_1);

                ensure!(
                    amount_0 >= amount_0_min && amount_1 >= amount_1_min,
                    Error::<T>::InsufficientTargetAmount
                );

                *total_liquidity = total_liquidity
                    .checked_sub(remove_liquidity)
                    .ok_or(Error::<T>::InsufficientLiquidity)?;
                Self::mutate_liquidity(asset_0, asset_1, who, remove_liquidity, false)?;

                T::MultiAssetsHandler::transfer(asset_0, &pair_account, recipient, amount_0)?;
                T::MultiAssetsHandler::transfer(asset_1, &pair_account, recipient, amount_1)?;

                Self::deposit_event(Event::LiquidityRemoved(
                    who.clone(),
                    recipient.clone(),
                    asset_0,
                    asset_1,
                    amount_0,
                    amount_1,
                    remove_liquidity,
                ));

                Ok(())
            } else {
                Err(Error::<T>::PairNotExists.into())
            }
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn inner_swap_exact_tokens_for_tokens(
        who: &T::AccountId,
        amount_in: AssetBalance,
        amount_out_min: AssetBalance,
        path: &[AssetId],
        recipient: &T::AccountId,
    ) -> DispatchResult {
        let amounts = Self::get_amount_out_by_path(amount_in, &path)?;
        ensure!(amounts[amounts.len() - 1] >= amount_out_min, Error::<T>::InsufficientTargetAmount);

        let pair_account =
            Self::get_pair_account_id(path[0], path[1]).ok_or(Error::<T>::PairNotExists)?;

        T::MultiAssetsHandler::transfer(path[0], who, &pair_account, amount_in)?;
        Self::swap(&amounts, &path, &recipient)?;

        Self::deposit_event(Event::AssetSwap(
            who.clone(),
            recipient.clone(),
            Vec::from(path),
            amount_in,
            amounts[amounts.len() - 1],
        ));

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn inner_swap_tokens_for_exact_tokens(
        who: &T::AccountId,
        amount_out: AssetBalance,
        amount_in_max: AssetBalance,
        path: &[AssetId],
        recipient: &T::AccountId,
    ) -> DispatchResult {
        let amounts = Self::get_amount_in_by_path(amount_out, &path)?;

        ensure!(amounts[0] <= amount_in_max, Error::<T>::ExcessiveSoldAmount);

        let pair_account =
            Self::get_pair_account_id(path[0], path[1]).ok_or(Error::<T>::PairNotExists)?;

        T::MultiAssetsHandler::transfer(path[0], who, &pair_account, amounts[0])?;
        Self::swap(&amounts, &path, recipient)?;

        Self::deposit_event(Event::AssetSwap(
            who.clone(),
            recipient.clone(),
            Vec::from(path),
            amounts[0],
            amount_out,
        ));

        Ok(())
    }

    fn calculate_share_amount(
        amount_0: AssetBalance,
        reserve_0: AssetBalance,
        reserve_1: AssetBalance,
    ) -> AssetBalance {
        U256::from(amount_0)
            .saturating_mul(U256::from(reserve_1))
            .checked_div(U256::from(reserve_0))
            .and_then(|n| TryInto::<AssetBalance>::try_into(n).ok())
            .unwrap_or_else(Zero::zero)
    }

    pub fn calculate_liquidity(
        amount_0: AssetBalance,
        amount_1: AssetBalance,
        reserve_0: AssetBalance,
        reserve_1: AssetBalance,
        total_liquidity: AssetBalance,
    ) -> AssetBalance {
        if total_liquidity == Zero::zero() {
            amount_0.saturating_mul(amount_1).integer_sqrt()
        } else {
            core::cmp::min(
                Self::calculate_share_amount(amount_0, reserve_0, total_liquidity),
                Self::calculate_share_amount(amount_1, reserve_1, total_liquidity),
            )
        }
    }

    pub fn calculate_added_amount(
        amount_0_desired: AssetBalance,
        amount_1_desired: AssetBalance,
        amount_0_min: AssetBalance,
        amount_1_min: AssetBalance,
        reserve_0: AssetBalance,
        reserve_1: AssetBalance,
    ) -> Result<(AssetBalance, AssetBalance), DispatchError> {
        if reserve_0 == Zero::zero() || reserve_1 == Zero::zero() {
            return Ok((amount_0_desired, amount_1_desired));
        }
        let amount_1_optimal = Self::calculate_share_amount(amount_0_desired, reserve_0, reserve_1);
        if amount_1_optimal <= amount_1_desired {
            ensure!(amount_1_optimal >= amount_1_min, Error::<T>::IncorrectAssetAmountRange);
            return Ok((amount_0_desired, amount_1_optimal));
        }
        let amount_0_optimal = Self::calculate_share_amount(amount_1_desired, reserve_1, reserve_0);
        ensure!(
            amount_0_optimal >= amount_0_min && amount_0_optimal <= amount_0_desired,
            Error::<T>::IncorrectAssetAmountRange
        );
        Ok((amount_0_optimal, amount_1_desired))
    }

    fn mutate_liquidity(
        asset_0: AssetId,
        asset_1: AssetId,
        who: &T::AccountId,
        amount: AssetBalance,
        is_mint: bool,
    ) -> DispatchResult {
        LiquidityLedger::<T>::try_mutate(
            (Self::sort_asset_id(asset_0, asset_1), who),
            |liquidity| {
                if is_mint {
                    *liquidity = liquidity.checked_add(amount).ok_or(Error::<T>::Overflow)?;
                } else {
                    *liquidity =
                        liquidity.checked_sub(amount).ok_or(Error::<T>::InsufficientLiquidity)?;
                }

                Ok(())
            },
        )
    }

    fn get_amount_in(
        output_amount: AssetBalance,
        input_reserve: AssetBalance,
        output_reserve: AssetBalance,
    ) -> AssetBalance {
        if input_reserve.is_zero() || output_reserve.is_zero() || output_amount.is_zero() {
            return Zero::zero();
        }

        let (fee_numerator, fee_denominator) = T::GetExchangeFee::get();

        let numerator = U256::from(input_reserve)
            .saturating_mul(U256::from(output_amount))
            .saturating_mul(U256::from(fee_denominator));

        let denominator = (U256::from(output_reserve).saturating_sub(U256::from(output_amount)))
            .saturating_mul(U256::from(fee_denominator.saturating_sub(fee_numerator)));

        numerator
            .checked_div(denominator)
            .and_then(|r| r.checked_add(U256::one()))
            .and_then(|n| TryInto::<AssetBalance>::try_into(n).ok())
            .unwrap_or_else(Zero::zero)
    }

    fn get_amount_out(
        input_amount: AssetBalance,
        input_reserve: AssetBalance,
        output_reserve: AssetBalance,
    ) -> AssetBalance {
        if input_reserve.is_zero() || output_reserve.is_zero() || input_amount.is_zero() {
            return Zero::zero();
        }

        let (fee_numerator, fee_denominator) = T::GetExchangeFee::get();

        let input_amount_with_fee =
            U256::from(input_amount.saturating_mul(
                fee_denominator.saturating_sub(fee_numerator).unique_saturated_into(),
            ));

        let numerator = input_amount_with_fee.saturating_mul(U256::from(output_reserve));

        let denominator = U256::from(input_reserve)
            .saturating_mul(U256::from(fee_denominator))
            .saturating_add(input_amount_with_fee);

        numerator
            .checked_div(denominator)
            .and_then(|n| TryInto::<AssetBalance>::try_into(n).ok())
            .unwrap_or_else(Zero::zero)
    }

    pub fn get_amount_in_by_path(
        amount_out: AssetBalance,
        path: &[AssetId],
    ) -> Result<Vec<AssetBalance>, DispatchError> {
        let len = path.len();
        ensure!(len > 1, Error::<T>::InvalidPath);

        let mut i = len - 1;
        let mut out_vec = vec![amount_out];

        while i > 0 {
            let pair_account = Self::pair_account_id(path[i], path[i - 1]);
            let reserve_0 = T::MultiAssetsHandler::balance_of(path[i], &pair_account);
            let reserve_1 = T::MultiAssetsHandler::balance_of(path[i - 1], &pair_account);

            ensure!(reserve_1 > Zero::zero() && reserve_0 > Zero::zero(), Error::<T>::InvalidPath);

            let amount = Self::get_amount_in(out_vec[len - 1 - i], reserve_1, reserve_0);
            ensure!(amount > One::one(), Error::<T>::InvalidPath);

            out_vec.push(amount);
            i -= 1;
        }

        out_vec.reverse();
        Ok(out_vec)
    }

    pub fn get_amount_out_by_path(
        amount_in: AssetBalance,
        path: &[AssetId],
    ) -> Result<Vec<AssetBalance>, DispatchError> {
        ensure!(path.len() > 1, Error::<T>::InvalidPath);

        let len = path.len() - 1;
        let mut out_vec = vec![amount_in];

        for i in 0..len {
            let pair_account = Self::pair_account_id(path[i], path[i + 1]);
            let reserve_0 = T::MultiAssetsHandler::balance_of(path[i], &pair_account);
            let reserve_1 = T::MultiAssetsHandler::balance_of(path[i + 1], &pair_account);

            ensure!(reserve_1 > Zero::zero() && reserve_0 > Zero::zero(), Error::<T>::InvalidPath);

            let amount = Self::get_amount_out(out_vec[i], reserve_0, reserve_1);
            ensure!(amount > Zero::zero(), Error::<T>::InvalidPath);
            out_vec.push(amount);
        }

        Ok(out_vec)
    }

    fn swap(
        amounts: &[AssetBalance],
        path: &[AssetId],
        recipient: &T::AccountId,
    ) -> DispatchResult {
        for i in 0..(amounts.len() - 1) {
            let input = path[i];
            let output = path[i + 1];
            let mut amount0_out: AssetBalance = AssetBalance::default();
            let mut amount1_out = amounts[i + 1];

            let (asset_0, asset_1) = Self::sort_asset_id(input, output);
            if input != asset_0 {
                amount0_out = amounts[i + 1];
                amount1_out = AssetBalance::default();
            }
            let pair_account =
                Self::get_pair_account_id(asset_0, asset_1).ok_or(Error::<T>::PairNotExists)?;

            if i < (amounts.len() - 2) {
                let mid_account = Self::get_pair_account_id(output, path[i + 2])
                    .ok_or(Error::<T>::PairNotExists)?;
                Self::pair_swap(
                    asset_0,
                    asset_1,
                    &pair_account,
                    amount0_out,
                    amount1_out,
                    &mid_account,
                )?;
            } else {
                Self::pair_swap(
                    asset_0,
                    asset_1,
                    &pair_account,
                    amount0_out,
                    amount1_out,
                    &recipient,
                )?;
            };
        }
        Ok(())
    }

    fn pair_swap(
        asset_0: AssetId,
        asset_1: AssetId,
        pair_account: &T::AccountId,
        amount_0: AssetBalance,
        amount_1: AssetBalance,
        recipient: &T::AccountId,
    ) -> DispatchResult {
        let reserve_0 = T::MultiAssetsHandler::balance_of(asset_0, &pair_account);
        let reserve_1 = T::MultiAssetsHandler::balance_of(asset_1, &pair_account);

        ensure!(
            amount_0 <= reserve_0 && amount_1 <= reserve_1,
            Error::<T>::InsufficientPairReserve
        );

        if amount_0 > Zero::zero() {
            T::MultiAssetsHandler::transfer(asset_0, &pair_account, recipient, amount_0)?;
        }

        if amount_1 > Zero::zero() {
            T::MultiAssetsHandler::transfer(asset_1, &pair_account, recipient, amount_1)?;
        }

        Ok(())
    }
}
