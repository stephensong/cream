#[cfg(feature = "contract")]
mod contract_impl {
    use cream_common::storefront::{StorefrontParameters, StorefrontState, StorefrontSummary};
    use freenet_stdlib::prelude::*;

    pub struct Contract;

    fn merge_validated(
        storefront: &mut StorefrontState,
        bytes: &[u8],
        owner: &ed25519_dalek::VerifyingKey,
    ) -> Result<(), ContractError> {
        if bytes.is_empty() {
            return Ok(());
        }
        let update: StorefrontState =
            serde_json::from_slice(bytes).map_err(|e| ContractError::Deser(e.to_string()))?;
        if !update.validate(owner) {
            return Err(ContractError::InvalidUpdate);
        }
        storefront.merge(update);
        Ok(())
    }

    #[contract]
    impl ContractInterface for Contract {
        fn validate_state(
            parameters: Parameters<'static>,
            state: State<'static>,
            _related: RelatedContracts<'static>,
        ) -> Result<ValidateResult, ContractError> {
            let bytes = state.as_ref();
            if bytes.is_empty() {
                return Ok(ValidateResult::Valid);
            }

            let params: StorefrontParameters = serde_json::from_slice(parameters.as_ref())
                .map_err(|e| ContractError::Deser(e.to_string()))?;

            let storefront: StorefrontState =
                serde_json::from_slice(bytes).map_err(|e| ContractError::Deser(e.to_string()))?;

            if !storefront.validate(&params.owner) {
                return Ok(ValidateResult::Invalid);
            }

            Ok(ValidateResult::Valid)
        }

        fn update_state(
            parameters: Parameters<'static>,
            state: State<'static>,
            data: Vec<UpdateData<'static>>,
        ) -> Result<UpdateModification<'static>, ContractError> {
            let params: StorefrontParameters = serde_json::from_slice(parameters.as_ref())
                .map_err(|e| ContractError::Deser(e.to_string()))?;

            let mut storefront: StorefrontState = if state.is_empty() {
                return Err(ContractError::Other(
                    "storefront must be initialized with state".into(),
                ));
            } else {
                serde_json::from_slice(state.as_ref())
                    .map_err(|e| ContractError::Deser(e.to_string()))?
            };

            for ud in data {
                match ud {
                    UpdateData::State(s) => {
                        merge_validated(&mut storefront, s.as_ref(), &params.owner)?;
                    }
                    UpdateData::Delta(d) => {
                        merge_validated(&mut storefront, d.as_ref(), &params.owner)?;
                    }
                    UpdateData::StateAndDelta { state, delta } => {
                        merge_validated(&mut storefront, state.as_ref(), &params.owner)?;
                        merge_validated(&mut storefront, delta.as_ref(), &params.owner)?;
                    }
                    _ => return Err(ContractError::InvalidUpdate),
                }
            }

            let serialized =
                serde_json::to_vec(&storefront).map_err(|e| ContractError::Other(e.to_string()))?;
            Ok(UpdateModification::valid(State::from(serialized)))
        }

        fn summarize_state(
            _parameters: Parameters<'static>,
            state: State<'static>,
        ) -> Result<StateSummary<'static>, ContractError> {
            if state.is_empty() {
                return Ok(StateSummary::from(vec![]));
            }

            let storefront: StorefrontState = serde_json::from_slice(state.as_ref())
                .map_err(|e| ContractError::Deser(e.to_string()))?;

            let summary = storefront.summarize();
            let serialized =
                serde_json::to_vec(&summary).map_err(|e| ContractError::Other(e.to_string()))?;
            Ok(StateSummary::from(serialized))
        }

        fn get_state_delta(
            _parameters: Parameters<'static>,
            state: State<'static>,
            summary: StateSummary<'static>,
        ) -> Result<StateDelta<'static>, ContractError> {
            if state.is_empty() {
                return Ok(StateDelta::from(vec![]));
            }

            let storefront: StorefrontState = serde_json::from_slice(state.as_ref())
                .map_err(|e| ContractError::Deser(e.to_string()))?;

            let summary: StorefrontSummary = if summary.is_empty() {
                StorefrontSummary::default()
            } else {
                serde_json::from_slice(summary.as_ref())
                    .map_err(|e| ContractError::Deser(e.to_string()))?
            };

            let delta = storefront.delta(&summary);
            let serialized =
                serde_json::to_vec(&delta).map_err(|e| ContractError::Other(e.to_string()))?;
            Ok(StateDelta::from(serialized))
        }
    }
}
