//! The decoded Plan proposal and the validated ExecutionPlan and Patch.
//! @sergent/docs/framework.md

use serde::ser::{Error as _, SerializeStruct};
use serde::{Serialize, Serializer};

use crate::operation::PlanStep;
use crate::run_record::CapturedValue;
use crate::scene::SceneIdentity;
use crate::target::Target;

/// The decoded model-proposed Operation script: typed Operations with their
/// framework-minted identities. It is proposal input to derivation, not the
/// ExecutionPlan. @sergent/docs/framework.md
pub struct PlanProposal<S, I, T: Target> {
    steps: Vec<PlanStep<S, I, T>>,
}

/// A typed Operation script derived from a Plan Proposal after a validated
/// continue Intent, bound to the observed Scene identity.
/// @sergent/docs/framework.md
pub struct ExecutionPlan<S, I, T: Target> {
    base: SceneIdentity,
    steps: Vec<PlanStep<S, I, T>>,
}

/// An ordered, replayable sequence of Operations compiled from a validated
/// ExecutionPlan. Its steps are isolated copies that retain the same Operation
/// IDs for trace matching. @sergent/docs/framework.md
pub struct Patch<S, I, T: Target> {
    base: SceneIdentity,
    steps: Vec<PlanStep<S, I, T>>,
}

impl<S, I, T: Target> PlanProposal<S, I, T> {
    /// Assemble the decoded steps inside the registry owner.
    pub(crate) fn decoded(steps: Vec<PlanStep<S, I, T>>) -> Self {
        Self { steps }
    }

    /// Borrow the decoded steps in model script order.
    pub fn steps(&self) -> &[PlanStep<S, I, T>] {
        &self.steps
    }

    /// Capture the typed proposal envelope by restoring each registry-owned
    /// call discriminator around the concrete operand fields.
    pub fn captured_value(&self) -> CapturedValue {
        CapturedValue::capture(self)
    }

    /// Consume the decoded proposal and bind all of its steps to the observed
    /// Scene identity without changing their order or identities.
    pub fn bind_to_scene(self, base: SceneIdentity) -> ExecutionPlan<S, I, T> {
        ExecutionPlan {
            base,
            steps: self.steps,
        }
    }
}

impl<S, I, T: Target> ExecutionPlan<S, I, T> {
    /// Borrow the Scene identity this Plan observed.
    pub fn base(&self) -> &SceneIdentity {
        &self.base
    }

    /// Borrow the validated steps in script order.
    pub fn steps(&self) -> &[PlanStep<S, I, T>] {
        &self.steps
    }

    /// Capture the derived Plan with its base identity and concrete Operation
    /// data, excluding framework Operation ids and call discriminators.
    pub fn captured_value(&self) -> CapturedValue {
        CapturedValue::capture(self)
    }

    /// Compile an isolated Patch while preserving every step identity and call.
    pub fn compile_isolated_patch(&self) -> Patch<S, I, T> {
        Patch {
            base: self.base.clone(),
            steps: self.steps.iter().map(PlanStep::isolated_copy).collect(),
        }
    }
}

impl<S, I, T: Target> Serialize for PlanProposal<S, I, T> {
    /// Restore every fixed call around concrete operand fields.
    fn serialize<Ser: Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        let operations = self
            .steps
            .iter()
            .map(PlanStep::projected_proposal_operation)
            .collect::<Result<Vec<_>, _>>()
            .map_err(Ser::Error::custom)?;
        let mut proposal = serializer.serialize_struct("PlanProposal", 1)?;
        proposal.serialize_field("operations", &operations)?;
        proposal.end()
    }
}

impl<S, I, T: Target> Serialize for ExecutionPlan<S, I, T> {
    /// Project base identity and concrete operand data without framework ids.
    fn serialize<Ser: Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        let steps = self
            .steps
            .iter()
            .map(PlanStep::projected_operation)
            .collect::<Result<Vec<_>, _>>()
            .map_err(Ser::Error::custom)?;
        let mut plan = serializer.serialize_struct("ExecutionPlan", 2)?;
        plan.serialize_field("base", &self.base)?;
        plan.serialize_field("steps", &steps)?;
        plan.end()
    }
}

impl<S, I, T: Target> Patch<S, I, T> {
    /// Borrow the Scene identity this Patch is based on.
    pub fn base(&self) -> &SceneIdentity {
        &self.base
    }

    /// Borrow the isolated Patch steps in replay order.
    pub fn steps(&self) -> &[PlanStep<S, I, T>] {
        &self.steps
    }

    /// Build a rebase result against a caller-supplied current Scene identity.
    ///
    /// Steps must come from existing isolated steps or
    /// [`PlanStep::with_operation`], so their framework identities and fixed
    /// calls cannot be changed. Runtime rebase validation owns the envelope,
    /// Target, and ordered identity checks. @sergent/docs/framework.md
    pub fn for_rebase(base: SceneIdentity, steps: Vec<PlanStep<S, I, T>>) -> Self {
        Self { base, steps }
    }
}
