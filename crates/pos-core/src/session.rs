use std::collections::BTreeMap;

use crate::error::CoreError;
use crate::money::Money;
use crate::payment::PaymentMethod;

/// Cobro agregado por medio de pago, tal como lo entrega el store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentTotal {
    pub method: PaymentMethod,
    pub amount: Money,
}

/// Datos de una sesión de caja antes de contar el efectivo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionTotals {
    pub opening_float: Money,
    pub ticket_count: u32,
    pub sales_net: Money,
    pub sales_tax: Money,
    pub sales_total: Money,
    /// Cobros de venta por medio. No incluye cashin/cashout.
    pub payments: Vec<PaymentTotal>,
    pub cash_in: Money,
    pub cash_out: Money,
    pub removed_lines: u32,
    pub drawer_openings: u32,
    pub no_sale_count: u32,
}

impl Default for SessionTotals {
    fn default() -> Self {
        Self {
            opening_float: Money::ZERO,
            ticket_count: 0,
            sales_net: Money::ZERO,
            sales_tax: Money::ZERO,
            sales_total: Money::ZERO,
            payments: Vec::new(),
            cash_in: Money::ZERO,
            cash_out: Money::ZERO,
            removed_lines: 0,
            drawer_openings: 0,
            no_sale_count: 0,
        }
    }
}

impl SessionTotals {
    pub fn collected_by_method(&self) -> BTreeMap<PaymentMethod, Money> {
        let mut totals = BTreeMap::new();
        for payment in &self.payments {
            *totals
                .entry(payment.method.clone())
                .or_insert(Money::ZERO) += payment.amount;
        }
        totals
    }

    /// Suma de todos los cobros de venta (todos los medios).
    pub fn collected_sales(&self) -> Money {
        self.payments.iter().map(|payment| payment.amount).sum()
    }

    pub fn cash_sales(&self) -> Money {
        self.payments
            .iter()
            .filter(|payment| payment.method.is_cash())
            .map(|payment| payment.amount)
            .sum()
    }

    /// Efectivo que debería haber en el cajón: fondo + ventas en efectivo +
    /// entradas − salidas.
    pub fn expected_cash(&self) -> Money {
        self.opening_float + self.cash_sales() + self.cash_in - self.cash_out
    }

    /// Diferencia entre lo vendido por líneas y lo cobrado. En datos nuevos debe
    /// ser cero; en el histórico hay 322 receipts descuadrados que se conservan.
    pub fn sales_difference(&self) -> Money {
        self.sales_total - self.collected_sales()
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        if self.opening_float.is_negative()
            || self.cash_in.is_negative()
            || self.cash_out.is_negative()
            || self.sales_net.is_negative()
            || self.sales_tax.is_negative()
            || self.sales_total.is_negative()
        {
            return Err(CoreError::InvalidSession(
                "los importes de la sesión no pueden ser negativos".to_owned(),
            ));
        }
        for payment in &self.payments {
            if payment.amount.is_negative() {
                return Err(CoreError::InvalidSession(
                    "un cobro por medio no puede ser negativo".to_owned(),
                ));
            }
        }
        Ok(())
    }

    /// Cierra la sesión con el efectivo contado.
    pub fn close(&self, counted_cash: Money) -> Result<SessionClose, CoreError> {
        self.validate()?;
        if counted_cash.is_negative() {
            return Err(CoreError::InvalidCashCount);
        }
        let expected_cash = self.expected_cash();
        let difference = counted_cash - expected_cash;
        Ok(SessionClose {
            totals: self.clone(),
            counted_cash,
            expected_cash,
            difference,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionClose {
    pub totals: SessionTotals,
    pub counted_cash: Money,
    pub expected_cash: Money,
    /// Positivo = sobrante; negativo = faltante.
    pub difference: Money,
}

impl SessionClose {
    pub fn is_balanced(&self) -> bool {
        self.difference.is_zero()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn money(value: i64) -> Money {
        Money::from_pesos(value)
    }

    fn totals() -> SessionTotals {
        SessionTotals {
            opening_float: money(50_000),
            ticket_count: 10,
            sales_net: money(120_000),
            sales_tax: money(0),
            sales_total: money(120_000),
            payments: vec![
                PaymentTotal {
                    method: PaymentMethod::Cash,
                    amount: money(100_000),
                },
                PaymentTotal {
                    method: PaymentMethod::Nequi,
                    amount: money(20_000),
                },
            ],
            cash_in: money(10_000),
            cash_out: money(30_000),
            removed_lines: 3,
            drawer_openings: 12,
            no_sale_count: 1,
        }
    }

    #[test]
    fn efectivo_esperado_incluye_fondo_y_movimientos() {
        let totals = totals();
        assert_eq!(totals.cash_sales(), money(100_000));
        assert_eq!(totals.expected_cash(), money(130_000));
    }

    #[test]
    fn cierre_con_faltante() {
        let close = totals().close(money(128_000)).unwrap();
        assert_eq!(close.expected_cash, money(130_000));
        assert_eq!(close.difference, money(-2_000));
        assert!(!close.is_balanced());
    }

    #[test]
    fn cierre_cuadrado() {
        let close = totals().close(money(130_000)).unwrap();
        assert!(close.is_balanced());
    }

    #[test]
    fn separa_ventas_de_cobros() {
        let mut totals = totals();
        totals.sales_total = money(125_000);
        assert_eq!(totals.sales_difference(), money(5_000));
    }

    #[test]
    fn rechaza_importes_negativos() {
        let mut totals = totals();
        totals.cash_out = money(-1);
        assert!(matches!(totals.validate(), Err(CoreError::InvalidSession(_))));
    }
}
