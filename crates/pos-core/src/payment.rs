use std::collections::BTreeMap;

use crate::error::CoreError;
use crate::money::Money;

/// Medios de pago del negocio. `Other` permite agregar uno nuevo sin tocar el
/// núcleo; los cinco históricos tienen variantes propias.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PaymentMethod {
    Cash,
    Nequi,
    Daviplata,
    Bancolombia,
    Magcard,
    Other(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaymentKind {
    Cash,
    Electronic,
    Card,
}

impl PaymentMethod {
    pub const CASH: Self = Self::Cash;

    pub fn from_code(code: &str) -> Self {
        match code.trim().to_ascii_lowercase().as_str() {
            "cash" => Self::Cash,
            "nequi" => Self::Nequi,
            "daviplata" => Self::Daviplata,
            "bancolombia" => Self::Bancolombia,
            "magcard" => Self::Magcard,
            other => Self::Other(other.to_owned()),
        }
    }

    pub fn code(&self) -> &str {
        match self {
            Self::Cash => "cash",
            Self::Nequi => "nequi",
            Self::Daviplata => "daviplata",
            Self::Bancolombia => "bancolombia",
            Self::Magcard => "magcard",
            Self::Other(code) => code.as_str(),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Cash => "Efectivo",
            Self::Nequi => "Nequi",
            Self::Daviplata => "Daviplata",
            Self::Bancolombia => "Bancolombia",
            Self::Magcard => "Tarjeta",
            Self::Other(code) => code.as_str(),
        }
    }

    pub fn kind(&self) -> PaymentKind {
        match self {
            Self::Cash => PaymentKind::Cash,
            Self::Nequi | Self::Daviplata | Self::Bancolombia => PaymentKind::Electronic,
            Self::Magcard => PaymentKind::Card,
            Self::Other(_) => PaymentKind::Electronic,
        }
    }

    pub fn is_cash(&self) -> bool {
        matches!(self, Self::Cash)
    }
}

/// Configuración editable de un medio de pago (semilla de `pos_payment_methods`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentMethodConfig {
    pub method: PaymentMethod,
    pub label: String,
    pub kind: PaymentKind,
    pub enabled: bool,
    pub requires_reference: bool,
    pub sort_order: u32,
}

impl PaymentMethodConfig {
    pub fn defaults() -> Vec<Self> {
        [
            (PaymentMethod::Cash, 0),
            (PaymentMethod::Nequi, 1),
            (PaymentMethod::Daviplata, 2),
            (PaymentMethod::Bancolombia, 3),
            (PaymentMethod::Magcard, 4),
        ]
        .into_iter()
        .map(|(method, sort_order)| Self {
            label: method.label().to_owned(),
            kind: method.kind(),
            enabled: true,
            requires_reference: false,
            sort_order,
            method,
        })
        .collect()
    }
}

/// Un pago aplicado al ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Payment {
    pub method: PaymentMethod,
    /// Importe aplicado al total (nunca negativo).
    pub amount: Money,
    /// Efectivo recibido del cliente. En medios electrónicos debe ser igual a
    /// `amount` o quedar en `None`.
    pub tendered: Option<Money>,
    /// Referencia/confirmación opcional (Nequi, Daviplata, Bancolombia).
    pub reference: Option<String>,
}

impl Payment {
    pub fn new(
        method: PaymentMethod,
        amount: Money,
        tendered: Option<Money>,
        reference: Option<String>,
    ) -> Result<Self, CoreError> {
        if !amount.is_positive() {
            return Err(CoreError::InvalidPayment(
                "el importe del pago debe ser mayor que cero".to_owned(),
            ));
        }

        let reference = reference.and_then(|value| {
            let trimmed = value.trim().to_owned();
            (!trimmed.is_empty()).then_some(trimmed)
        });
        if let Some(value) = &reference {
            if value.chars().count() > 80 {
                return Err(CoreError::TextTooLong { max: 80 });
            }
        }

        let tendered = match method.kind() {
            PaymentKind::Cash => {
                let received = tendered.unwrap_or(amount);
                if received < amount {
                    return Err(CoreError::CashTenderedTooLow);
                }
                Some(received)
            }
            _ => {
                if let Some(received) = tendered {
                    if received != amount {
                        return Err(CoreError::InvalidPayment(
                            "un pago electrónico no puede recibir más que su importe".to_owned(),
                        ));
                    }
                }
                None
            }
        };

        Ok(Self {
            method,
            amount,
            tendered,
            reference,
        })
    }

    pub fn cash(amount: Money, tendered: Money) -> Result<Self, CoreError> {
        Self::new(PaymentMethod::Cash, amount, Some(tendered), None)
    }

    pub fn electronic(method: PaymentMethod, amount: Money, reference: Option<String>) -> Result<Self, CoreError> {
        Self::new(method, amount, None, reference)
    }

    pub fn is_cash(&self) -> bool {
        self.method.is_cash()
    }
}

/// Lista de pagos de un ticket.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Payments {
    entries: Vec<Payment>,
}

impl Payments {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, payment: Payment) {
        self.entries.push(payment);
    }

    pub fn pop(&mut self) -> Option<Payment> {
        self.entries.pop()
    }

    pub fn entries(&self) -> &[Payment] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn has_cash(&self) -> bool {
        self.entries.iter().any(Payment::is_cash)
    }

    pub fn total_paid(&self) -> Money {
        self.entries.iter().map(|p| p.amount).sum()
    }

    pub fn by_method(&self) -> BTreeMap<PaymentMethod, Money> {
        let mut totals = BTreeMap::new();
        for payment in &self.entries {
            *totals.entry(payment.method.clone()).or_insert(Money::ZERO) += payment.amount;
        }
        totals
    }

    /// Valida los pagos contra el total y calcula la devuelta.
    pub fn summary(&self, total: Money) -> Result<PaymentSummary, CoreError> {
        if total.is_negative() {
            return Err(CoreError::InvalidPayment(
                "el total del ticket no puede ser negativo".to_owned(),
            ));
        }

        let mut non_cash_paid = Money::ZERO;
        let mut cash_paid = Money::ZERO;
        let mut cash_tendered = Money::ZERO;

        for payment in &self.entries {
            if payment.is_cash() {
                cash_paid += payment.amount;
                cash_tendered += payment.tendered.unwrap_or(payment.amount);
            } else {
                non_cash_paid += payment.amount;
            }
        }

        if non_cash_paid > total {
            return Err(CoreError::NonCashOverpay);
        }

        let cash_due = total - non_cash_paid;
        if cash_paid < cash_due {
            return Err(CoreError::InsufficientPayment(cash_due - cash_paid));
        }
        if cash_tendered < cash_paid {
            return Err(CoreError::CashTenderedTooLow);
        }

        let paid = non_cash_paid + cash_paid;
        let change = cash_tendered - cash_due;
        let remaining = if paid < total { total - paid } else { Money::ZERO };

        Ok(PaymentSummary {
            total,
            paid,
            cash_paid,
            cash_tendered,
            non_cash_paid,
            change,
            remaining,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentSummary {
    pub total: Money,
    pub paid: Money,
    pub cash_paid: Money,
    pub cash_tendered: Money,
    pub non_cash_paid: Money,
    pub change: Money,
    pub remaining: Money,
}

impl PaymentSummary {
    pub fn is_complete(&self) -> bool {
        self.remaining.is_zero() && !self.change.is_negative()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn money(value: i64) -> Money {
        Money::from_pesos(value)
    }

    #[test]
    fn efectivo_exacto() {
        let mut payments = Payments::new();
        payments.add(Payment::cash(money(10_000), money(10_000)).unwrap());
        let summary = payments.summary(money(10_000)).unwrap();
        assert_eq!(summary.change, Money::ZERO);
        assert_eq!(summary.remaining, Money::ZERO);
        assert!(summary.is_complete());
    }

    #[test]
    fn efectivo_con_devuelta() {
        let mut payments = Payments::new();
        payments.add(Payment::cash(money(10_000), money(20_000)).unwrap());
        let summary = payments.summary(money(10_000)).unwrap();
        assert_eq!(summary.change, money(10_000));
        assert_eq!(summary.paid, money(10_000));
    }

    #[test]
    fn pago_mixto() {
        let mut payments = Payments::new();
        payments.add(Payment::electronic(PaymentMethod::Nequi, money(6_000), None).unwrap());
        payments.add(Payment::cash(money(4_000), money(5_000)).unwrap());
        let summary = payments.summary(money(10_000)).unwrap();
        assert_eq!(summary.non_cash_paid, money(6_000));
        assert_eq!(summary.cash_paid, money(4_000));
        assert_eq!(summary.change, money(1_000));
        assert_eq!(summary.paid, money(10_000));
    }

    #[test]
    fn efectivo_que_cubre_todo_y_paga_de_mas() {
        let mut payments = Payments::new();
        payments.add(Payment::cash(money(10_000), money(12_000)).unwrap());
        let summary = payments.summary(money(10_000)).unwrap();
        assert_eq!(summary.change, money(2_000));
        assert_eq!(summary.paid, money(10_000));
    }

    #[test]
    fn efectivo_por_encima_del_total_tambien_genera_devuelta() {
        let mut payments = Payments::new();
        payments.add(Payment::cash(money(12_000), money(12_000)).unwrap());
        let summary = payments.summary(money(10_000)).unwrap();
        assert_eq!(summary.change, money(2_000));
        assert_eq!(summary.paid, money(12_000));
    }

    #[test]
    fn rechaza_pago_insuficiente() {
        let mut payments = Payments::new();
        payments.add(Payment::cash(money(3_000), money(3_000)).unwrap());
        assert_eq!(
            payments.summary(money(10_000)),
            Err(CoreError::InsufficientPayment(money(7_000)))
        );
    }

    #[test]
    fn rechaza_electronico_por_encima_del_total() {
        let mut payments = Payments::new();
        payments.add(Payment::electronic(PaymentMethod::Magcard, money(11_000), None).unwrap());
        assert_eq!(payments.summary(money(10_000)), Err(CoreError::NonCashOverpay));
    }

    #[test]
    fn electronico_no_acepta_recibido_distinto() {
        let err = Payment::new(
            PaymentMethod::Nequi,
            money(10_000),
            Some(money(12_000)),
            None,
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::InvalidPayment(_)));
    }

    #[test]
    fn medios_de_pago_historicos() {
        let configs = PaymentMethodConfig::defaults();
        assert_eq!(configs.len(), 5);
        assert_eq!(configs[0].method.code(), "cash");
        assert_eq!(PaymentMethod::from_code("nequi"), PaymentMethod::Nequi);
        assert_eq!(PaymentMethod::from_code("otro").code(), "otro");
        assert_eq!(PaymentMethod::from_code("otro").label(), "otro");
    }

    #[test]
    fn total_por_metodo() {
        let mut payments = Payments::new();
        payments.add(Payment::cash(money(4_000), money(4_000)).unwrap());
        payments.add(Payment::cash(money(1_000), money(1_000)).unwrap());
        payments.add(Payment::electronic(PaymentMethod::Nequi, money(5_000), None).unwrap());
        let totals = payments.by_method();
        assert_eq!(totals[&PaymentMethod::Cash], money(5_000));
        assert_eq!(totals[&PaymentMethod::Nequi], money(5_000));
        assert_eq!(payments.total_paid(), money(10_000));
    }
}
