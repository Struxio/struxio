// SPDX-License-Identifier: AGPL-3.0-only

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionsError;

impl fmt::Display for InstructionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "instruction text must not be empty")
    }
}

impl std::error::Error for InstructionsError {}

/// Provider-neutral extraction instructions. No model or vendor names belong here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderNeutralInstructions {
    task: String,
    context: Vec<String>,
    output_requirements: Vec<String>,
}

impl ProviderNeutralInstructions {
    pub fn new(task: impl Into<String>) -> Result<Self, InstructionsError> {
        let task = task.into();
        if task.trim().is_empty() {
            return Err(InstructionsError);
        }
        Ok(Self {
            task,
            context: Vec::new(),
            output_requirements: Vec::new(),
        })
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Result<Self, InstructionsError> {
        let context = context.into();
        if context.trim().is_empty() {
            return Err(InstructionsError);
        }
        self.context.push(context);
        Ok(self)
    }

    pub fn with_output_requirement(
        mut self,
        requirement: impl Into<String>,
    ) -> Result<Self, InstructionsError> {
        let requirement = requirement.into();
        if requirement.trim().is_empty() {
            return Err(InstructionsError);
        }
        self.output_requirements.push(requirement);
        Ok(self)
    }

    pub fn task(&self) -> &str {
        &self.task
    }

    pub fn context(&self) -> &[String] {
        &self.context
    }

    pub fn output_requirements(&self) -> &[String] {
        &self.output_requirements
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_blank_task() {
        assert!(ProviderNeutralInstructions::new("   ").is_err());
        assert!(ProviderNeutralInstructions::new("Extract the invoice fields.").is_ok());
    }
}
