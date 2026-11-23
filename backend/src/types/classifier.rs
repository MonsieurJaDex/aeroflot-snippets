//! Text classifier that maps a free-form problem description to an engineer role.
//!
//! Training data is a TSV file (`description<TAB>engineer_type`). The model is a
//! Naive Bayes classifier from the `bayesian` crate, weighted with TF-IDF.
//! The [`AircraftIssue`] enum stays the source of truth for known issue types;
//! this module only covers free text that does not match one of those variants.

use std::{fs, path::Path};

use anyhow::{Context, anyhow, bail};
use bayesian::Classifier;

use crate::types::enums::EngineerType;

/// Predicted engineer role and the TF-IDF Naive Bayes confidence for that class.
#[derive(Debug, Clone, Copy)]
pub struct Classification {
    pub engineer_type: EngineerType,
    pub confidence: f64,
}

/// Trained problem-description classifier.
pub struct ProblemClassifier {
    model: Classifier<EngineerType>,
}

impl std::fmt::Debug for ProblemClassifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProblemClassifier")
            .field("learned", &self.model.learned())
            .field("tfidf", &self.model.has_tfidf())
            .finish()
    }
}

impl ProblemClassifier {
    /// Learns every row of the TSV dataset and builds TF-IDF weights.
    pub fn train_from_dataset(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read classifier dataset {}", path.display()))?;

        let mut model = Classifier::new(vec![
            EngineerType::IntegrityInspector,
            EngineerType::CrewRemarksHandler,
            EngineerType::FuelingCrew,
            EngineerType::EngineTechnician,
            EngineerType::AvionicsEngineer,
            EngineerType::AviationTechnician,
        ]);

        let mut learned = 0usize;
        for (line_no, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (text, label) = line.split_once('\t').with_context(|| {
                format!(
                    "{}:{}: expected 'text<TAB>engineer_type'",
                    path.display(),
                    line_no + 1
                )
            })?;
            let engineer_type = parse_engineer_type(label).with_context(|| {
                format!(
                    "{}:{}: unknown label '{label}'",
                    path.display(),
                    line_no + 1
                )
            })?;
            let tokens = tokenize(text);
            if tokens.is_empty() {
                bail!(
                    "{}:{}: description has no words",
                    path.display(),
                    line_no + 1
                );
            }
            model.learn(&tokens, &engineer_type);
            learned += 1;
        }

        if learned == 0 {
            bail!("classifier dataset {} has no rows", path.display());
        }

        model.build_tfidf();
        tracing::info!(documents = learned, path = %path.display(), "problem classifier trained");
        Ok(Self { model })
    }

    /// Classifies free text with TF-IDF weighted Naive Bayes.
    pub fn classify(&self, text: &str) -> anyhow::Result<Classification> {
        let tokens = tokenize(text);
        if tokens.is_empty() {
            return Err(anyhow!("description has no words"));
        }

        let engineer_type = *self.model.classify_tfidf(&tokens);
        let probs = self.model.prob_scores_tfidf(&tokens);
        let confidence = self
            .model
            .classes()
            .iter()
            .position(|class| *class == engineer_type)
            .and_then(|index| probs.get(index).copied())
            .unwrap_or(0.0);

        dbg!(&tokens, &engineer_type, &confidence);

        Ok(Classification {
            engineer_type,
            confidence,
        })
    }

    /// Number of training documents currently stored in the model.
    #[must_use]
    pub fn learned(&self) -> usize {
        self.model.learned()
    }
}

/// Splits a description into lowercase words, keeping Cyrillic and Latin letters.
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn parse_engineer_type(label: &str) -> anyhow::Result<EngineerType> {
    serde_json::from_value(serde_json::Value::String(label.trim().to_owned()))
        .with_context(|| format!("'{label}' is not an engineer type"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_inertial_drift_as_avionics() {
        let model =
            ProblemClassifier::train_from_dataset("assets/problem_classification_dataset.tsv")
                .expect("dataset should train");
        let result = model
            .classify("уход гироплатформы ИНС, курс смещается")
            .expect("text should classify");
        assert_eq!(result.engineer_type, EngineerType::AvionicsEngineer);
        assert!(result.confidence > 0.5);
        assert!(model.learned() > 0);
    }
}
