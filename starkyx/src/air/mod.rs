pub mod curta_air;
pub mod extension;
pub mod opening;
pub mod parser;

#[cfg(test)]
pub mod fibonacci;

use parser::AirParser;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RoundDatum {
    /// The number of columns generated in this round
    pub num_columns: usize,
    /// The range of global values generated in this round
    pub global_values_range: (usize, usize),
    /// The number of validator challenges needed after this round
    pub num_challenges: usize,
}

pub trait AirConstraint<AP: AirParser> {
    /// Evaluation of the vanishing polynomials.
    fn eval(&self, parser: &mut AP);
}

pub trait RAirData {
    /// The width of the AIR data
    fn width(&self) -> usize;

    /// The maximal constraint degree
    fn constraint_degree(&self) -> usize;

    /// The data needed for each round
    fn round_data(&self) -> Vec<RoundDatum>;

    /// Total number of columns across all rounds
    fn num_columns(&self) -> usize {
        self.round_data().iter().map(|d| d.num_columns).sum()
    }

    /// Number of public inputs
    fn num_public_inputs(&self) -> usize;

    /// Total number of rounds
    fn num_rounds(&self) -> usize {
        self.round_data().len()
    }

    /// Total number of global values across all rounds
    fn num_global_values(&self) -> usize {
        self.round_data()
            .iter()
            .map(|d| d.global_values_range.1 - d.global_values_range.0)
            .sum()
    }

    /// Quotient degree factor
    fn quotient_degree_factor(&self) -> usize {
        1.max(self.constraint_degree().saturating_sub(1))
    }
}

pub trait RAir<AP: AirParser>: RAirData {
    /// Evaluation of the vanishing polynomials.
    fn eval(&self, parser: &mut AP);

    /// Evaluation of global vanishing constraints
    fn eval_global(&self, parser: &mut AP);
}

impl RoundDatum {
    pub fn new(
        num_columns: usize,
        global_values_range: (usize, usize),
        num_challenges: usize,
    ) -> Self {
        Self {
            num_columns,
            global_values_range,
            num_challenges,
        }
    }
}
