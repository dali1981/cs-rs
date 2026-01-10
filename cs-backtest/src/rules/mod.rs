//! Rule evaluation for entry filtering
//!
//! Evaluates entry rules from RulesConfig at each stage of the backtest pipeline.

mod evaluator;

pub use evaluator::RuleEvaluator;
