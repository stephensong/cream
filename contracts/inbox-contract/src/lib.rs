#[cfg(feature = "contract")]
mod contract_impl {
    use cream_common::inbox::{InboxParameters, InboxState, InboxSummary};
    use freenet_stdlib::prelude::*;

    pub struct Contract;

    fn merge_validated(
        state: &mut InboxState,
        bytes: &[u8],
    ) -> Result<(), ContractError> {
        if bytes.is_empty() {
            return Ok(());
        }
        let update: InboxState =
            serde_json::from_slice(bytes).map_err(|e| ContractError::Deser(e.to_string()))?;
        if !state.validate_update(&update) {
            return Err(ContractError::InvalidUpdate);
        }
        state.merge(update);
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

            let params: InboxParameters = serde_json::from_slice(parameters.as_ref())
                .map_err(|e| ContractError::Deser(e.to_string()))?;

            let inbox_state: InboxState =
                serde_json::from_slice(bytes).map_err(|e| ContractError::Deser(e.to_string()))?;

            if !inbox_state.validate(&params.owner) {
                return Ok(ValidateResult::Invalid);
            }

            Ok(ValidateResult::Valid)
        }

        fn update_state(
            parameters: Parameters<'static>,
            state: State<'static>,
            data: Vec<UpdateData<'static>>,
        ) -> Result<UpdateModification<'static>, ContractError> {
            let _params: InboxParameters = serde_json::from_slice(parameters.as_ref())
                .map_err(|e| ContractError::Deser(e.to_string()))?;

            let mut inbox_state: InboxState = if state.is_empty() {
                return Err(ContractError::Other(
                    "inbox contract must be initialized with state".into(),
                ));
            } else {
                serde_json::from_slice(state.as_ref())
                    .map_err(|e| ContractError::Deser(e.to_string()))?
            };

            for ud in data {
                match ud {
                    UpdateData::State(s) => {
                        merge_validated(&mut inbox_state, s.as_ref())?;
                    }
                    UpdateData::Delta(d) => {
                        merge_validated(&mut inbox_state, d.as_ref())?;
                    }
                    UpdateData::StateAndDelta { state, delta } => {
                        merge_validated(&mut inbox_state, state.as_ref())?;
                        merge_validated(&mut inbox_state, delta.as_ref())?;
                    }
                    _ => return Err(ContractError::InvalidUpdate),
                }
            }

            let serialized =
                serde_json::to_vec(&inbox_state).map_err(|e| ContractError::Other(e.to_string()))?;
            Ok(UpdateModification::valid(State::from(serialized)))
        }

        fn summarize_state(
            _parameters: Parameters<'static>,
            state: State<'static>,
        ) -> Result<StateSummary<'static>, ContractError> {
            if state.is_empty() {
                return Ok(StateSummary::from(vec![]));
            }

            let inbox_state: InboxState = serde_json::from_slice(state.as_ref())
                .map_err(|e| ContractError::Deser(e.to_string()))?;

            let summary = inbox_state.summarize();
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

            let inbox_state: InboxState = serde_json::from_slice(state.as_ref())
                .map_err(|e| ContractError::Deser(e.to_string()))?;

            let summary: InboxSummary = if summary.is_empty() {
                InboxSummary::default()
            } else {
                serde_json::from_slice(summary.as_ref())
                    .map_err(|e| ContractError::Deser(e.to_string()))?
            };

            let delta_bytes = match inbox_state.delta(&summary) {
                Some(delta) => {
                    serde_json::to_vec(&delta).map_err(|e| ContractError::Other(e.to_string()))?
                }
                None => vec![],
            };
            Ok(StateDelta::from(delta_bytes))
        }
    }
}
