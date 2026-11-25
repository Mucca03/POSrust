use std::fmt;
use std::iter::Sum;
use std::ops::{Add, Neg, Sub};
use std::str::FromStr;

use rust_decimal::{Decimal, RoundingStrategy};

use crate::error::CoreError;

/// Unidad de venta de un producto.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SaleUnit {
    /// Se vende por unidades enteras.
    Unit,
    /// Se vende por peso en kilogramos (3 decimales: gramos).
    Kilogram,
}

impl SaleUnit {
    pub const fn is_weighted(self) -> bool {
        matches!(self, Self::Kilogram)
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::Unit => "unit",
            Self::Kilogram => "kg",
        }
    }
}

/// Cantidad con escala 3. Puede ser negativa para movimientos de stock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Qty(Decimal);

pub const QTY_SCALE: u32 = 3;

impl Qty {
    pub const ZERO: Self = Self(Decimal::ZERO);

    pub fn from_decimal(value: Decimal) -> Self {
        Self(value.round_dp_with_strategy(QTY_SCALE, RoundingStrategy::MidpointAwayFromZero))
    }

    pub fn from_units(units: i64) -> Self {
        Self(Decimal::from(units))
    }

    pub fn from_grams(grams: i64) -> Self {
        Self(Decimal::new(grams, 3))
    }

    pub fn parse(value: &str) -> Result<Self, CoreError> {
        let trimmed = value.trim().replace(',', ".");
        if trimmed.is_empty() {
            return Err(CoreError::InvalidQuantity(value.to_owned()));
        }
        let parsed = Decimal::from_str(&trimmed)
            .map_err(|_| CoreError::InvalidQuantity(value.to_owned()))?;
        Ok(Self::from_decimal(parsed))
    }

    pub fn one() -> Self {
        Self(Decimal::ONE)
    }

    pub fn zero() -> Self {
        Self::ZERO
    }

    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    pub fn is_positive(self) -> bool {
        self.0 > Decimal::ZERO
    }

    pub fn is_negative(self) -> bool {
        self.0 < Decimal::ZERO
    }

    pub fn is_integer(self) -> bool {
        self.0.fract().is_zero()
    }

    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    pub fn as_decimal(self) -> Decimal {
        self.0
    }

    pub fn to_plain_string(self) -> String {
        self.0.normalize().to_string()
    }

    /// `0,850 kg` para pesables; `2 ud` para unidades.
    pub fn format(self, unit: SaleUnit) -> String {
        match unit {
            SaleUnit::Kilogram => {
                let value = self
                    .0
                    .round_dp_with_strategy(QTY_SCALE, RoundingStrategy::MidpointAwayFromZero);
                format!("{} kg", fixed_decimals(value, QTY_SCALE as usize).replace('.', ","))
            }
            SaleUnit::Unit => {
                let value = self.0.normalize().to_string().replace('.', ",");
                format!("{value} ud")
            }
        }
    }
}

fn fixed_decimals(value: Decimal, decimals: usize) -> String {
    let mut text = value.to_string();
    if let Some(dot) = text.find('.') {
        let current = text.len() - dot - 1;
        if current < decimals {
            text.push_str(&"0".repeat(decimals - current));
        } else if current > decimals {
            text.truncate(dot + 1 + decimals);
        }
    } else {
        text.push('.');
        text.push_str(&"0".repeat(decimals));
    }
    text
}

impl Add for Qty {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::from_decimal(self.0 + rhs.0)
    }
}

impl Sub for Qty {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::from_decimal(self.0 - rhs.0)
    }
}

impl Neg for Qty {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

impl Sum for Qty {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, Add::add)
    }
}

impl fmt::Display for Qty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.normalize().to_string())
    }
}

impl FromStr for Qty {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn redondea_a_tres_decimales() {
        assert_eq!(Qty::from_decimal(dec!(1.2345)).as_decimal(), dec!(1.235));
        assert_eq!(Qty::parse("0,850").unwrap().as_decimal(), dec!(0.850));
    }

    #[test]
    fn formatea_pesables_y_unidades() {
        assert_eq!(Qty::from_grams(850).format(SaleUnit::Kilogram), "0,850 kg");
        assert_eq!(Qty::from_units(2).format(SaleUnit::Unit), "2 ud");
        assert_eq!(Qty::from_units(1).format(SaleUnit::Kilogram), "1,000 kg");
    }

    #[test]
    fn permite_negativos_para_stock() {
        let q = -Qty::from_grams(500);
        assert!(q.is_negative());
        assert_eq!(q.as_decimal(), dec!(-0.500));
    }

    #[test]
    fn suma_de_cantidades() {
        let total: Qty = vec![Qty::from_grams(500), Qty::from_grams(350)]
            .into_iter()
            .sum();
        assert_eq!(total.as_decimal(), dec!(0.850));
    }
}
