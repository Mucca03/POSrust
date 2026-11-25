use std::collections::BTreeMap;

use crate::error::CoreError;
use crate::money::Money;

/// Denominaciones colombianas usadas en la caja.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Denomination {
    D100000,
    D50000,
    D20000,
    D10000,
    D5000,
    D2000,
    D1000,
    D500,
    D200,
    D100,
    D50,
}

impl Denomination {
    pub const ALL: [Self; 11] = [
        Self::D100000,
        Self::D50000,
        Self::D20000,
        Self::D10000,
        Self::D5000,
        Self::D2000,
        Self::D1000,
        Self::D500,
        Self::D200,
        Self::D100,
        Self::D50,
    ];

    pub const fn value_cop(self) -> i64 {
        match self {
            Self::D100000 => 100_000,
            Self::D50000 => 50_000,
            Self::D20000 => 20_000,
            Self::D10000 => 10_000,
            Self::D5000 => 5_000,
            Self::D2000 => 2_000,
            Self::D1000 => 1_000,
            Self::D500 => 500,
            Self::D200 => 200,
            Self::D100 => 100,
            Self::D50 => 50,
        }
    }

    pub fn value(self) -> Money {
        Money::from_pesos(self.value_cop())
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::D100000 => "100.000",
            Self::D50000 => "50.000",
            Self::D20000 => "20.000",
            Self::D10000 => "10.000",
            Self::D5000 => "5.000",
            Self::D2000 => "2.000",
            Self::D1000 => "1.000",
            Self::D500 => "500",
            Self::D200 => "200",
            Self::D100 => "100",
            Self::D50 => "50",
        }
    }
}

/// Conteo de efectivo por denominación.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CashCount {
    counts: BTreeMap<Denomination, u32>,
}

impl CashCount {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, denomination: Denomination, count: u32) {
        if count == 0 {
            self.counts.remove(&denomination);
        } else {
            self.counts.insert(denomination, count);
        }
    }

    pub fn add(&mut self, denomination: Denomination, count: u32) -> Result<(), CoreError> {
        let current = self.counts.get(&denomination).copied().unwrap_or(0);
        let total = current
            .checked_add(count)
            .ok_or(CoreError::InvalidCashCount)?;
        self.set(denomination, total);
        Ok(())
    }

    pub fn count(&self, denomination: Denomination) -> u32 {
        self.counts.get(&denomination).copied().unwrap_or(0)
    }

    pub fn total(&self) -> Money {
        let pesos = self
            .counts
            .iter()
            .map(|(denomination, count)| denomination.value_cop() * i64::from(*count))
            .sum();
        Money::from_pesos(pesos)
    }

    pub fn entries(&self) -> impl Iterator<Item = (Denomination, u32)> + '_ {
        self.counts.iter().map(|(denomination, count)| (*denomination, *count))
    }

    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    pub fn clear(&mut self) {
        self.counts.clear();
    }
}

/// Desglose de una devuelta. Si el importe no es múltiplo de 50, `remainder`
/// indica la parte que no se puede entregar con las denominaciones mínimas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeBreakdown {
    pub parts: Vec<(Denomination, u32)>,
    pub remainder: Money,
    pub amount: Money,
}

impl ChangeBreakdown {
    pub fn total_in_parts(&self) -> Money {
        self.parts
            .iter()
            .map(|(denomination, count)| {
                Money::from_pesos(denomination.value_cop() * i64::from(*count))
            })
            .sum()
    }
}

/// Calcula la devuelta con la menor cantidad de billetes/monedas.
///
/// Las denominaciones colombianas son canónicas para el algoritmo voraz.
pub fn change_breakdown(amount: Money) -> Result<ChangeBreakdown, CoreError> {
    if amount.is_negative() {
        return Err(CoreError::InvalidPayment(
            "la devuelta no puede ser negativa".to_owned(),
        ));
    }

    let cents = amount.to_cents();
    let minimum_cents = 50 * 100;
    let remainder_cents = cents % minimum_cents;
    let mut remaining = cents - remainder_cents;

    let mut parts = Vec::new();
    for denomination in Denomination::ALL {
        let value = denomination.value_cop() * 100;
        let count = remaining / value;
        if count > 0 {
            parts.push((denomination, u32::try_from(count).unwrap_or(u32::MAX)));
            remaining -= count * value;
        }
    }

    Ok(ChangeBreakdown {
        parts,
        remainder: Money::from_cents(remainder_cents),
        amount,
    })
}

/// Redondeo de efectivo configurable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CashRounding {
    #[default]
    None,
    Nearest50,
    Nearest100,
}

impl CashRounding {
    pub const fn increment_cop(self) -> i64 {
        match self {
            Self::None => 0,
            Self::Nearest50 => 50,
            Self::Nearest100 => 100,
        }
    }

    pub fn from_cop(value: i64) -> Result<Self, CoreError> {
        match value {
            0 => Ok(Self::None),
            50 => Ok(Self::Nearest50),
            100 => Ok(Self::Nearest100),
            _ => Err(CoreError::InvalidRounding),
        }
    }

    /// Devuelve `(total_redondeado, ajuste)`.
    pub fn apply(self, total: Money) -> Result<(Money, Money), CoreError> {
        let increment = self.increment_cop();
        if increment == 0 || total.is_zero() {
            return Ok((total, Money::ZERO));
        }
        let rounded = total.round_to_increment(Money::from_pesos(increment))?;
        Ok((rounded, rounded - total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suma_denominaciones() {
        let mut count = CashCount::new();
        count.set(Denomination::D50000, 2);
        count.set(Denomination::D20000, 1);
        count.set(Denomination::D1000, 3);
        count.set(Denomination::D50, 2);
        assert_eq!(count.total(), Money::from_pesos(123_100));
        assert_eq!(count.count(Denomination::D50000), 2);
    }

    #[test]
    fn desglosa_devuelta_exacta() {
        let change = change_breakdown(Money::from_pesos(4_650)).unwrap();
        assert_eq!(change.remainder, Money::ZERO);
        assert_eq!(change.total_in_parts(), Money::from_pesos(4_650));
        assert_eq!(
            change.parts,
            vec![
                (Denomination::D2000, 2),
                (Denomination::D500, 1),
                (Denomination::D100, 1),
                (Denomination::D50, 1),
            ]
        );
    }

    #[test]
    fn informa_la_parte_no_representable() {
        let change = change_breakdown(Money::from_pesos(367)).unwrap();
        assert_eq!(change.remainder, Money::from_pesos(17));
        assert_eq!(change.total_in_parts(), Money::from_pesos(350));
    }

    #[test]
    fn redondeo_de_efectivo() {
        let total = Money::from_pesos(9_233);
        let (rounded, adjustment) = CashRounding::Nearest50.apply(total).unwrap();
        assert_eq!(rounded, Money::from_pesos(9_250));
        assert_eq!(adjustment, Money::from_pesos(17));

        let (rounded, _) = CashRounding::None.apply(total).unwrap();
        assert_eq!(rounded, total);

        assert_eq!(CashRounding::from_cop(50).unwrap(), CashRounding::Nearest50);
        assert!(CashRounding::from_cop(30).is_err());
    }

    #[test]
    fn redondeo_a_cien() {
        let (rounded, adjustment) = CashRounding::Nearest100
            .apply(Money::from_pesos(9_250))
            .unwrap();
        assert_eq!(rounded, Money::from_pesos(9_300));
        assert_eq!(adjustment, Money::from_pesos(50));
    }
}
