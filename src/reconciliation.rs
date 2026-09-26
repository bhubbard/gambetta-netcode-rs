use crate::client::{ClientInput, InputBuffer};

/// Result diagnostics from a reconciliation execution.
#[derive(Debug, Clone, PartialEq)]
pub struct ReconciliationResult<S> {
    /// Whether any re-simulation or snap occurred.
    pub reconciled: bool,
    /// Number of stale inputs discarded ($\le S_{srv}$).
    pub discarded_inputs: usize,
    /// Number of remaining inputs re-simulated ($> S_{srv}$).
    pub reapplied_inputs: usize,
    /// Distance divergence between pre-reconciliation predicted state and the post-reconciliation state.
    pub prediction_error: f32,
    /// Final reconciled predicted state.
    pub corrected_state: S,
}

/// Reconciles the client's predicted state with the authoritative server state:
/// 1. Evaluates divergence error between old predicted state and server state.
/// 2. Discards all inputs $\le \text{server\_seq}$.
/// 3. Snaps predicted state to `server_state`.
/// 4. Replays all inputs in `input_buffer` ($> \text{server\_seq}$) sequentially through `apply_fn`.
pub fn reconcile_state<S, A, F, DistFn>(
    predicted_state: &mut S,
    server_state: &S,
    server_seq: u64,
    input_buffer: &mut InputBuffer<A>,
    apply_fn: &F,
    distance_fn: &DistFn,
) -> ReconciliationResult<S>
where
    S: Clone,
    A: Clone,
    F: Fn(&mut S, &ClientInput<A>),
    DistFn: Fn(&S, &S) -> f32,
{
    // Discard all inputs <= server_seq
    let discarded = input_buffer.discard_acknowledged(server_seq);

    // Save previous prediction for error analysis
    let old_predicted = predicted_state.clone();

    // Snap to authoritative server state
    *predicted_state = server_state.clone();

    // Replay remaining inputs
    let mut reapplied = 0;
    for input in input_buffer.unacknowledged() {
        apply_fn(predicted_state, input);
        reapplied += 1;
    }

    let error = distance_fn(&old_predicted, predicted_state);

    ReconciliationResult {
        reconciled: true,
        discarded_inputs: discarded,
        reapplied_inputs: reapplied,
        prediction_error: error,
        corrected_state: predicted_state.clone(),
    }
}
