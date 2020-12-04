// Copyright 2020 Parity Technologies (UK) Ltd.
#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::{
	decl_event, decl_module, decl_storage,
	dispatch::DispatchResult,
	traits::{Currency, ExistenceRequirement, WithdrawReason},
};
use frame_system::ensure_signed;

use codec::{Decode, Encode};
use cumulus_primitives::{
	relay_chain::DownwardMessage,
	xcmp::{XCMPMessageHandler, XCMPMessageSender},
	DownwardMessageHandler, ParaId, UpwardMessageOrigin, UpwardMessageSender,
};
use cumulus_upward_message::BalancesMessage;
use polkadot_parachain::primitives::AccountIdConversion;
#[cfg(feature = "std")]
use serde::{Deserialize, Serialize};
use sp_core::{Hasher, H160, H256, U256};
use sp_std::{marker::PhantomData, vec::Vec};

#[derive(Encode, Decode)]
pub enum XCMPMessage<XAccountId, XBalance> {
	/// Transfer tokens to the given account from the Parachain account.
	TransferToken(XAccountId, XBalance),
}

type BalanceOf<T> =
	<<T as Trait>::Currency as Currency<<T as frame_system::Trait>::AccountId>>::Balance;

/// Configuration trait of this pallet.
pub trait Trait: frame_system::Trait {
	/// Event type used by the runtime.
	type Event: From<Event<Self>> + Into<<Self as frame_system::Trait>::Event>;

	/// The sender of upward messages.
	type UpwardMessageSender: UpwardMessageSender<Self::UpwardMessage>;

	/// The upward message type used by the Parachain runtime.
	type UpwardMessage: codec::Codec + BalancesMessage<Self::AccountId, BalanceOf<Self>>;

	/// Currency of the runtime.
	type Currency: Currency<Self::AccountId>;

	/// The sender of XCMP messages.
	type XCMPMessageSender: XCMPMessageSender<XCMPMessage<Self::AccountId, BalanceOf<Self>>>;

	/// Convert account ID to H160;
	type ConvertAccountId: ConvertAccountId<Self::AccountId>;
}

pub trait ConvertAccountId<A> {
	  /// Given a Substrate address, return the corresponding Ethereum address.
    fn convert_account_id(account_id: &A) -> H160;
}
/// Hash and then truncate the account id, taking the last 160-bit as the Ethereum address.
pub struct HashTruncateConvertAccountId<H>(PhantomData<H>);

impl<H: Hasher> Default for HashTruncateConvertAccountId<H> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<H: Hasher, A: AsRef<[u8]>> ConvertAccountId<A> for HashTruncateConvertAccountId<H> {
    fn convert_account_id(account_id: &A) -> H160 {
        let account_id = H::hash(account_id.as_ref());
        let account_id_len = account_id.as_ref().len();
        let mut value = [0u8; 20];
        let value_len = value.len();

        if value_len > account_id_len {
            value[(value_len - account_id_len)..].copy_from_slice(account_id.as_ref());
        } else {
            value.copy_from_slice(&account_id.as_ref()[(account_id_len - value_len)..]);
        }

        H160::from(value)
    }
}
#[derive(Clone, Eq, PartialEq, Encode, Decode, Default)]
#[cfg_attr(feature = "std", derive(Debug, Serialize, Deserialize))]
/// Ethereum account nonce, balance and code. Used by storage.
pub struct Account {
    /// Account nonce.
    pub nonce: U256,
    /// Account balance.
    pub balance: U256,
}

decl_storage! {
  trait Store for Module<T: Trait> as SSVM {
		Accounts get(fn accounts) config(): map hasher(blake2_128_concat) H160 => Account;
		AccountCodes: map hasher(blake2_128_concat) H160 => Vec<u8>;
		AccountStorages: double_map hasher(blake2_128_concat) H160, hasher(blake2_128_concat) H256 => H256;
  }
	add_extra_genesis {
		config(storage): Vec<(T::AccountId, H256, H256)>;
		build(|config: &GenesisConfig<T>| {
			for &(ref account, pos, value) in config.storage.iter() {
				AccountStorages::insert(T::ConvertAccountId::convert_account_id(&account), pos, value);
			}
		});
	}
}

decl_event! {
	pub enum Event<T> where
		AccountId = <T as frame_system::Trait>::AccountId,
		Balance = BalanceOf<T>
	{
		/// Transferred tokens to the account on the relay chain.
		TransferredTokensToRelayChain(AccountId, Balance),
		/// Transferred tokens to the account on request from the relay chain.
		TransferredTokensFromRelayChain(AccountId, Balance),
		/// Transferred tokens to the account from the given parachain account.
		TransferredTokensViaXCMP(ParaId, AccountId, Balance, DispatchResult),
	}
}

decl_module! {
	pub struct Module<T: Trait> for enum Call where origin: T::Origin {
		/// Transfer `amount` of tokens on the relay chain from the Parachain account to
		/// the given `dest` account.
		#[weight = 10]
		fn transfer_tokens_to_relay_chain(origin, dest: T::AccountId, amount: BalanceOf<T>) {
			let who = ensure_signed(origin)?;

			let _ = T::Currency::withdraw(
				&who,
				amount,
				WithdrawReason::Transfer.into(),
				ExistenceRequirement::AllowDeath,
			)?;

			let msg = <T as Trait>::UpwardMessage::transfer(dest.clone(), amount.clone());
			<T as Trait>::UpwardMessageSender::send_upward_message(&msg, UpwardMessageOrigin::Signed)
				.expect("Should not fail; qed");

			#[cfg(feature = "std")]
			{
				// Alice account
				let raw: Vec<u8> = rustc_hex::FromHex::from_hex("d43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d").unwrap_or_default();
				let account : T::AccountId = T::AccountId::decode(&mut &raw[..]).unwrap_or_default();
				T::Currency::deposit_creating(&account, 10.into());
			}

			Self::deposit_event(Event::<T>::TransferredTokensToRelayChain(dest, amount));
		}

		/// Transfer `amount` of tokens to another parachain.
		#[weight = 10]
		fn transfer_tokens_to_parachain_chain(
			origin,
			para_id: u32,
			dest: T::AccountId,
			amount: BalanceOf<T>,
		) {
			//TODO we don't make sure that the parachain has some tokens on the other parachain.
			let who = ensure_signed(origin)?;

			let _ = T::Currency::withdraw(
				&who,
				amount,
				WithdrawReason::Transfer.into(),
				ExistenceRequirement::AllowDeath,
			)?;

			T::XCMPMessageSender::send_xcmp_message(
				para_id.into(),
				&XCMPMessage::TransferToken(dest, amount),
			).expect("Should not fail; qed");
		}

		fn deposit_event() = default;
	}
}

/// This is a hack to convert from one generic type to another where we are sure that both are the
/// same type/use the same encoding.
fn convert_hack<O: Decode>(input: &impl Encode) -> O {
	input.using_encoded(|e| Decode::decode(&mut &e[..]).expect("Must be compatible; qed"))
}

impl<T: Trait> DownwardMessageHandler for Module<T> {
	fn handle_downward_message(msg: &DownwardMessage) {
		match msg {
			DownwardMessage::TransferInto(dest, amount, _) => {
				let dest = convert_hack(&dest);
				let amount: BalanceOf<T> = convert_hack(amount);

				let _ = T::Currency::deposit_creating(&dest, amount.clone());

				Self::deposit_event(Event::<T>::TransferredTokensFromRelayChain(dest, amount));
			}
			_ => {}
		}
	}
}

impl<T: Trait> XCMPMessageHandler<XCMPMessage<T::AccountId, BalanceOf<T>>> for Module<T> {
	fn handle_xcmp_message(src: ParaId, msg: &XCMPMessage<T::AccountId, BalanceOf<T>>) {
		match msg {
			XCMPMessage::TransferToken(dest, amount) => {
				let para_account = src.clone().into_account();

				let res = T::Currency::transfer(
					&para_account,
					dest,
					amount.clone(),
					ExistenceRequirement::AllowDeath,
				);

				Self::deposit_event(Event::<T>::TransferredTokensViaXCMP(
					src,
					dest.clone(),
					amount.clone(),
					res,
				));
			}
		}
	}
}
