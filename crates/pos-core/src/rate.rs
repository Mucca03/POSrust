use std::fmt;
use std::str::FromStr;

use rust_decimal::{Decimal, RoundingStrategy};

use crate::error::CoreError;

/// Tasa como fracción: `0.19` representa 19 %.
///
/// Se usa para impuestos y descuentos porcentuales. El rango permitido es
/// `0..=1`; un descuento del 100 % es válido, uno superior no.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rate(Decimal);

pub const RATE_SCALE: u32 = 6;

impl Rate {
    pub const ZERO: Self = Self(Decimal::ZERO);

    pub fn from_fraction(value: Decimal) -> Result<Self, CoreError> {
        let rounded = value.round_dp_with_strategy(RATE_SCALE, RoundingStrategy::MidpointAwayFromZero);
        if rounded < Decimal::ZERO || rounded > Decimal::ONE {
            return Err(CoreError::InvalidRate(value.to_string()));
        }
        Ok(Self(rounded))
    }

    pub fn from_percent(percent: Decimal) -> Result<Self, CoreError> {
        Self::from_fraction(percent / Decimal::ONE_HUNDRED)
    }

    pub fn parse_percent(value: &str) -> Result<Self, CoreError> {
        let trimmed = value.trim().trim_end_matches('%').trim().replace(',', ".");
        if trimmed.is_empty() {
            return Err(CoreError::InvalidRate(value.to_owned()));
        }
        let parsed = Decimal::from_str(&trimmed)
            .map_err(|_| CoreError::InvalidRate(value.to_owned()))?;
        Self::from_percent(parsed)
    }

    pub fn zero() -> Self {
        Self::ZERO
    }

    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    pub fn as_decimal(self) -> Decimal {
        self.0
    }

    pub fn as_percent(self) -> Decimal {
        self.0 * Decimal::ONE_HUNDRED
    }

    /// Multiplicador para convertir neto en bruto: `1 + tasa`.
    pub fn multiplier(self) -> Decimal {
        Decimal::ONE + self.0
    }

    pub fn format_percent(self) -> String {
        format!("{}%", self.as_percent().normalize())
    }
}

impl fmt::Display for Rate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format_percent())
    }
}

impl FromStr for Rate {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_percent(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn convierte_porcentaje_a_fraccion() {
        let rate = Rate::parse_percent("19").unwrap();
        assert_eq!(rate.as_decimal(), dec!(0.19));
        assert_eq!(rate.as_percent(), dec!(19));
        assert_eq!(rate.multiplier(), dec!(1.19));
        assert_eq!(rate.format_percent(), "19%");
    }

    #[test]
    fn acepta_coma_y_signo_de_porcentaje() {
        assert_eq!(Rate::parse_percent("5,5%").unwrap().as_decimal(), dec!(0.055));
    }

    #[test]
    fn rechaza_fuera_de_rango() {
        assert!(Rate::parse_percent("-1").is_err());
        assert!(Rate::parse_percent("101").is_err());
        assert!(Rate::parse_percent("").is_err());
    }
}
