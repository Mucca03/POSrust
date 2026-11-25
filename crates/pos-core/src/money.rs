use std::fmt;
use std::iter::Sum;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};
use std::str::FromStr;

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy};

use crate::error::CoreError;
use crate::quantity::Qty;

/// Importe monetario con escala 2 y redondeo `MidpointAwayFromZero`.
///
/// Es `Copy`, inmutable y no usa coma flotante. El valor puede ser negativo
/// (descuentos, ajustes y salidas de caja).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Money(Decimal);

pub const MONEY_SCALE: u32 = 2;

impl Money {
    pub const ZERO: Self = Self(Decimal::ZERO);

    /// Redondea a 2 decimales con la regla del negocio.
    pub fn from_decimal(value: Decimal) -> Self {
        Self(value.round_dp_with_strategy(MONEY_SCALE, RoundingStrategy::MidpointAwayFromZero))
    }

    pub fn from_cents(cents: i64) -> Self {
        Self(Decimal::new(cents, 2))
    }

    pub fn from_pesos(pesos: i64) -> Self {
        Self(Decimal::from(pesos))
    }

    pub fn parse(value: &str) -> Result<Self, CoreError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(CoreError::InvalidMoney(value.to_owned()));
        }
        let parsed = Decimal::from_str(trimmed)
            .map_err(|_| CoreError::InvalidMoney(value.to_owned()))?;
        Ok(Self::from_decimal(parsed))
    }

    pub const fn zero() -> Self {
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

    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    pub fn as_decimal(self) -> Decimal {
        self.0
    }

    /// Centavos exactos. El POS maneja importes muy por debajo de `i64::MAX`.
    pub fn to_cents(self) -> i64 {
        (self.0 * Decimal::ONE_HUNDRED)
            .round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero)
            .to_i64()
            .expect("un importe del POS siempre cabe en i64")
    }

    pub fn to_plain_string(self) -> String {
        self.0.normalize().to_string()
    }

    /// Formato de pantalla/tirilla: `$ 1.234.567` (COP sin decimales).
    pub fn format_cop(self) -> String {
        let rounded = self
            .0
            .round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero);
        Self::format_grouped_cop(rounded, false)
    }

    /// Muestra centavos solo cuando el importe tiene una parte fraccionaria.
    pub fn format_cop_exact(self) -> String {
        Self::format_grouped_cop(self.0, !self.0.fract().is_zero())
    }

    fn format_grouped_cop(value: Decimal, with_cents: bool) -> String {
        let negative = value.is_sign_negative() && !value.is_zero();
        let abs = value.abs();
        let raw = abs.to_string();
        let (integer, fraction) = raw.split_once('.').unwrap_or((&raw, ""));

        let mut grouped = String::with_capacity(integer.len() + integer.len() / 3);
        for (index, ch) in integer.chars().rev().enumerate() {
            if index > 0 && index % 3 == 0 {
                grouped.push('.');
            }
            grouped.push(ch);
        }
        let grouped: String = grouped.chars().rev().collect();
        let sign = if negative { "-" } else { "" };
        if with_cents {
            format!("$ {sign}{grouped},{fraction:0<2}")
        } else {
            format!("$ {sign}{grouped}")
        }
    }

    /// Formato con decimales exactos, útil para CSV y depuración.
    pub fn format_with_decimals(self, decimals: u32) -> String {
        let value = self
            .0
            .round_dp_with_strategy(decimals, RoundingStrategy::MidpointAwayFromZero);
        value.to_string().replace('.', ",")
    }

    /// Redondea al múltiplo más cercano (50 o 100 COP, por ejemplo).
    pub fn round_to_increment(self, increment: Self) -> Result<Self, CoreError> {
        if !increment.is_positive() {
            return Err(CoreError::InvalidRounding);
        }
        let steps = (self.0 / increment.0)
            .round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero);
        Ok(Self::from_decimal(steps * increment.0))
    }
}

impl Add for Money {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::from_decimal(self.0 + rhs.0)
    }
}

impl Sub for Money {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::from_decimal(self.0 - rhs.0)
    }
}

impl Neg for Money {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

impl AddAssign for Money {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl SubAssign for Money {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl std::ops::Mul<Qty> for Money {
    type Output = Self;

    fn mul(self, rhs: Qty) -> Self::Output {
        Self::from_decimal(self.0 * rhs.as_decimal())
    }
}

impl Sum for Money {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, Add::add)
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format_cop())
    }
}

impl FromStr for Money {
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
    fn redondea_a_dos_decimales_alejandose_del_cero() {
        assert_eq!(Money::from_decimal(dec!(1.005)).as_decimal(), dec!(1.01));
        assert_eq!(Money::from_decimal(dec!(-1.005)).as_decimal(), dec!(-1.01));
        assert_eq!(Money::from_decimal(dec!(2.344)).as_decimal(), dec!(2.34));
    }

    #[test]
    fn formatea_pesos_colombianos() {
        assert_eq!(Money::from_pesos(0).format_cop(), "$ 0");
        assert_eq!(Money::from_pesos(999).format_cop(), "$ 999");
        assert_eq!(Money::from_pesos(1_000).format_cop(), "$ 1.000");
        assert_eq!(Money::from_pesos(1_234_567).format_cop(), "$ 1.234.567");
        assert_eq!(Money::from_pesos(-4_600).format_cop(), "$ -4.600");
    }

    #[test]
    fn parsea_y_rechaza_texto_invalido() {
        assert_eq!(Money::parse(" 4600 ").unwrap(), Money::from_pesos(4_600));
        assert_eq!(Money::parse("4600.50").unwrap().as_decimal(), dec!(4600.50));
        assert!(Money::parse("").is_err());
        assert!(Money::parse("abc").is_err());
    }

    #[test]
    fn redondeo_de_efectivo() {
        let total = Money::from_pesos(9_233);
        let rounded = total.round_to_increment(Money::from_pesos(50)).unwrap();
        assert_eq!(rounded, Money::from_pesos(9_250));
        assert_eq!(rounded - total, Money::from_pesos(17));

        let total = Money::from_pesos(9_225);
        let rounded = total.round_to_increment(Money::from_pesos(50)).unwrap();
        assert_eq!(rounded, Money::from_pesos(9_250));

        assert!(total.round_to_increment(Money::ZERO).is_err());
    }

    #[test]
    fn suma_y_resta_sin_perder_centavos() {
        let a = Money::from_cents(10);
        let b = Money::from_cents(20);
        assert_eq!(a + b, Money::from_cents(30));
        assert_eq!(b - a, Money::from_cents(10));
        assert_eq!(-a, Money::from_cents(-10));
        assert_eq!(vec![a, b].into_iter().sum::<Money>(), Money::from_cents(30));
    }

    #[test]
    fn texto_para_base_de_datos_es_decimal_plano() {
        assert_eq!(Money::from_pesos(4_600).to_plain_string(), "4600");
        assert_eq!(Money::from_cents(123).to_plain_string(), "1.23");
        assert_eq!(Money::from_pesos(-4_600).to_plain_string(), "-4600");
    }
}
