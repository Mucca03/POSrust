//! Información de una venta confirmada para consulta e impresión.

use crate::{Money, PaymentMethod, Qty, SaleUnit};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub id: String,
    pub number: i64,
    pub date: String,
    pub cashier: String,
    pub lines: Vec<ReceiptLine>,
    pub payments: Vec<ReceiptPayment>,
    pub net: Money,
    pub tax: Money,
    pub total: Money,
    pub collected: Money,
    pub change: Money,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptLine {
    pub name: String,
    pub code: Option<String>,
    pub unit_price_net: Money,
    pub quantity: Qty,
    pub unit: SaleUnit,
    pub amount: Money,
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptPayment {
    pub method: PaymentMethod,
    pub amount: Money,
    pub tendered: Option<Money>,
    pub reference: Option<String>,
}
