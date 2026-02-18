#[cfg(feature = "contract")]
mod contract_impl {
    use cream_common::directory::{DirectoryState, DirectorySummary};
    use freenet_stdlib::prelude::*;

    pub struct Contract;

    fn merge_validated(directory: &mut DirectoryState, bytes: &[u8]) -> Result<(), ContractError> {
        if bytes.is_empty() {
            return Ok(());
        }
        let update: DirectoryState =
            serde_json::from_slice(bytes).map_err(|e| ContractError::Deser(e.to_string()))?;
        if !update.validate_all_signatures() {
            return Err(ContractError::InvalidUpdate);
        }
        directory.merge(update);
        Ok(())
    }

    #[contract]
    impl ContractInterface for Contract {
        fn validate_state(
            _parameters: Parameters<'static>,
            state: State<'static>,
            _related: RelatedContracts<'static>,
        ) -> Result<ValidateResult, ContractError> {
            let bytes = state.as_ref();
            if bytes.is_empty() {
                return Ok(ValidateResult::Valid);
            }

            let directory: DirectoryState =
                serde_json::from_slice(bytes).map_err(|e| ContractError::Deser(e.to_string()))?;

            if !directory.validate_all_signatures() {
                return Ok(ValidateResult::Invalid);
            }

            Ok(ValidateResult::Valid)
        }

        fn update_state(
            _parameters: Parameters<'static>,
            state: State<'static>,
            data: Vec<UpdateData<'static>>,
        ) -> Result<UpdateModification<'static>, ContractError> {
            let mut directory = if state.is_empty() {
                DirectoryState::default()
            } else {
                serde_json::from_slice(state.as_ref())
                    .map_err(|e| ContractError::Deser(e.to_string()))?
            };

            for ud in data {
                match ud {
                    UpdateData::State(s) => {
                        merge_validated(&mut directory, s.as_ref())?;
                    }
                    UpdateData::Delta(d) => {
                        merge_validated(&mut directory, d.as_ref())?;
                    }
                    UpdateData::StateAndDelta { state, delta } => {
                        merge_validated(&mut directory, state.as_ref())?;
                        merge_validated(&mut directory, delta.as_ref())?;
                    }
                    _ => return Err(ContractError::InvalidUpdate),
                }
            }

            let serialized =
                serde_json::to_vec(&directory).map_err(|e| ContractError::Other(e.to_string()))?;
            Ok(UpdateModification::valid(State::from(serialized)))
        }

        fn summarize_state(
            _parameters: Parameters<'static>,
            state: State<'static>,
        ) -> Result<StateSummary<'static>, ContractError> {
            if state.is_empty() {
                return Ok(StateSummary::from(vec![]));
            }

            let directory: DirectoryState = serde_json::from_slice(state.as_ref())
                .map_err(|e| ContractError::Deser(e.to_string()))?;

            let summary = directory.summarize();
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

            let directory: DirectoryState = serde_json::from_slice(state.as_ref())
                .map_err(|e| ContractError::Deser(e.to_string()))?;

            let summary: DirectorySummary = if summary.is_empty() {
                DirectorySummary::default()
            } else {
                serde_json::from_slice(summary.as_ref())
                    .map_err(|e| ContractError::Deser(e.to_string()))?
            };

            let delta = directory.delta(&summary);
            let serialized =
                serde_json::to_vec(&delta).map_err(|e| ContractError::Other(e.to_string()))?;
            Ok(StateDelta::from(serialized))
        }
    }
}
