use std::collections::BTreeMap;

use crate::cash::CashRounding;
use crate::error::CoreError;
use crate::ids::{LineKey, ProductId, TaxCategoryId};
use crate::model::{PriceMode, Product, ProductSnapshot, TAX_CATEGORY_SIN_IVA};
use crate::money::Money;
use crate::payment::{PaymentSummary, Payments};
use crate::quantity::Qty;
use crate::rate::Rate;

const MAX_NOTE_CHARS: usize = 80;

/// Descuento porcentual o de importe fijo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Discount {
    Percent(Rate),
    Amount(Money),
}

impl Discount {
    pub fn percent(rate: Rate) -> Self {
        Self::Percent(rate)
    }

    pub fn amount(amount: Money) -> Self {
        Self::Amount(amount)
    }

    pub fn is_zero(self) -> bool {
        match self {
            Self::Percent(rate) => rate.is_zero(),
            Self::Amount(amount) => amount.is_zero(),
        }
    }

    pub fn percent_value(self) -> Option<Rate> {
        match self {
            Self::Percent(rate) => Some(rate),
            Self::Amount(_) => None,
        }
    }

    pub fn amount_value(self) -> Option<Money> {
        match self {
            Self::Percent(_) => None,
            Self::Amount(amount) => Some(amount),
        }
    }

    /// Importe del descuento sobre una base no negativa.
    pub fn amount_for(self, base: Money) -> Result<Money, CoreError> {
        if base.is_negative() {
            return Err(CoreError::InvalidDiscount(
                "la base del descuento no puede ser negativa".to_owned(),
            ));
        }
        match self {
            Self::Percent(rate) => {
                Ok(Money::from_decimal(base.as_decimal() * rate.as_decimal()))
            }
            Self::Amount(amount) => {
                if amount.is_negative() {
                    return Err(CoreError::InvalidDiscount(
                        "el descuento no puede ser negativo".to_owned(),
                    ));
                }
                if amount > base {
                    return Err(CoreError::DiscountExceedsBase);
                }
                Ok(amount)
            }
        }
    }
}

/// Línea lógica del carrito. Los descuentos se guardan como datos de la línea
/// y se expanden a líneas negativas al persistir.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketLine {
    key: LineKey,
    product: ProductSnapshot,
    quantity: Qty,
    note: Option<String>,
    discount: Option<Discount>,
    original_unit_price_net: Option<Money>,
}

impl TicketLine {
    pub fn key(&self) -> LineKey {
        self.key
    }

    pub fn product(&self) -> &ProductSnapshot {
        &self.product
    }

    pub fn quantity(&self) -> Qty {
        self.quantity
    }

    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    pub fn discount(&self) -> Option<Discount> {
        self.discount
    }

    pub fn original_unit_price_net(&self) -> Option<Money> {
        self.original_unit_price_net
    }

    pub fn net_before_discount(&self) -> Result<Money, CoreError> {
        Ok(self.product.unit_price_net * self.quantity)
    }

    pub fn discount_amount(&self) -> Result<Money, CoreError> {
        let base = self.net_before_discount()?;
        match self.discount {
            Some(discount) => discount.amount_for(base),
            None => Ok(Money::ZERO),
        }
    }

    pub fn net(&self) -> Result<Money, CoreError> {
        Ok(self.net_before_discount()? - self.discount_amount()?)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineAmount {
    pub key: LineKey,
    pub net_before_discount: Money,
    pub discount: Money,
    pub net: Money,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaxAmount {
    pub tax_category_id: TaxCategoryId,
    pub rate: Rate,
    pub base: Money,
    pub tax: Money,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscountAllocation {
    pub tax_category_id: TaxCategoryId,
    pub amount: Money,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketTotals {
    pub line_count: usize,
    pub net_before_discounts: Money,
    pub line_discounts: Money,
    pub ticket_discount: Money,
    pub total_discounts: Money,
    pub net: Money,
    pub tax: Money,
    pub total_before_rounding: Money,
    pub rounding_adjustment: Money,
    pub total: Money,
    pub tax_breakdown: Vec<TaxAmount>,
    pub ticket_discount_allocations: Vec<DiscountAllocation>,
    pub line_amounts: Vec<LineAmount>,
}

/// Carrito en construcción.
#[derive(Clone, Debug)]
pub struct TicketDraft {
    lines: Vec<TicketLine>,
    ticket_discount: Option<Discount>,
    cash_rounding: CashRounding,
    next_key: u64,
}

impl Default for TicketDraft {
    fn default() -> Self {
        Self::new()
    }
}

impl TicketDraft {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            ticket_discount: None,
            cash_rounding: CashRounding::None,
            next_key: 0,
        }
    }

    pub fn lines(&self) -> &[TicketLine] {
        &self.lines
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn ticket_discount(&self) -> Option<Discount> {
        self.ticket_discount
    }

    pub fn cash_rounding(&self) -> CashRounding {
        self.cash_rounding
    }

    pub fn set_cash_rounding(&mut self, rounding: CashRounding) {
        self.cash_rounding = rounding;
    }

    pub fn add_product(
        &mut self,
        product: &Product,
        quantity: Qty,
        price_mode: PriceMode,
    ) -> Result<LineKey, CoreError> {
        self.add_snapshot(product.snapshot(price_mode), quantity)
    }

    pub fn add_snapshot(
        &mut self,
        snapshot: ProductSnapshot,
        quantity: Qty,
    ) -> Result<LineKey, CoreError> {
        self.validate_quantity(snapshot.unit, quantity)?;
        if snapshot.unit_price_net.is_negative() {
            return Err(CoreError::InvalidMoney(
                "el precio unitario no puede ser negativo".to_owned(),
            ));
        }

        if let Some(index) = self.lines.iter().position(|line| {
            line.product.product_id == snapshot.product_id
                && line.product.unit_price_net == snapshot.unit_price_net
                && line.product.tax_category_id == snapshot.tax_category_id
                && line.product.tax_rate == snapshot.tax_rate
                && line.product.unit == snapshot.unit
                && line.note.is_none()
                && line.discount.is_none()
                && line.original_unit_price_net.is_none()
        }) {
            let old_quantity = self.lines[index].quantity;
            let new_quantity = Qty::from_decimal(old_quantity.as_decimal() + quantity.as_decimal());
            self.lines[index].quantity = new_quantity;
            if let Err(error) = self.totals() {
                self.lines[index].quantity = old_quantity;
                return Err(error);
            }
            return Ok(self.lines[index].key);
        }

        let key = LineKey::new(self.next_key);
        self.next_key += 1;
        self.lines.push(TicketLine {
            key,
            product: snapshot,
            quantity,
            note: None,
            discount: None,
            original_unit_price_net: None,
        });
        if let Err(error) = self.totals() {
            self.lines.pop();
            return Err(error);
        }
        Ok(key)
    }

    pub fn set_quantity(&mut self, key: LineKey, quantity: Qty) -> Result<(), CoreError> {
        self.edit_line(key, quantity, None)
    }

    /// Cambia cantidad y, opcionalmente, precio sin IVA en una sola operación.
    pub fn edit_line(
        &mut self, key: LineKey, quantity: Qty, price: Option<Money>,
    ) -> Result<(), CoreError> {
        let index = self.index_of(key)?;
        let line = &self.lines[index];
        self.validate_quantity(line.product.unit, quantity)?;
        if price.is_some_and(Money::is_negative) {
            return Err(CoreError::InvalidMoney("el precio unitario no puede ser negativo".into()));
        }
        let old = (line.quantity, line.product.unit_price_net, line.original_unit_price_net);
        let line = &mut self.lines[index];
        line.quantity = quantity;
        if let Some(price) = price {
            let original = old.2.unwrap_or(old.1);
            line.original_unit_price_net = (price != original).then_some(original);
            line.product.unit_price_net = price;
        }
        if let Err(error) = self.totals() {
            let line = &mut self.lines[index];
            (line.quantity, line.product.unit_price_net, line.original_unit_price_net) = old;
            return Err(error);
        }
        Ok(())
    }

    pub fn set_note(&mut self, key: LineKey, note: Option<&str>) -> Result<(), CoreError> {
        let index = self.index_of(key)?;
        let value = note
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        if let Some(value) = &value {
            if value.chars().count() > MAX_NOTE_CHARS {
                return Err(CoreError::TextTooLong {
                    max: MAX_NOTE_CHARS,
                });
            }
        }
        self.lines[index].note = value;
        Ok(())
    }

    pub fn set_line_discount(
        &mut self,
        key: LineKey,
        discount: Option<Discount>,
    ) -> Result<(), CoreError> {
        let index = self.index_of(key)?;
        let discount = discount.filter(|value| !value.is_zero());
        if let Some(value) = discount {
            let base = self.lines[index].net_before_discount()?;
            value.amount_for(base)?;
        }
        let old = self.lines[index].discount;
        self.lines[index].discount = discount;
        if let Err(error) = self.totals() {
            self.lines[index].discount = old;
            return Err(error);
        }
        Ok(())
    }

    pub fn set_ticket_discount(&mut self, discount: Option<Discount>) -> Result<(), CoreError> {
        let discount = discount.filter(|value| !value.is_zero());
        let old = self.ticket_discount;
        self.ticket_discount = discount;
        if let Err(error) = self.totals() {
            self.ticket_discount = old;
            return Err(error);
        }
        Ok(())
    }

    pub fn remove_line(&mut self, key: LineKey) -> Result<RemovedLine, CoreError> {
        let index = self.index_of(key)?;
        let line = self.lines.remove(index);
        if let Err(error) = self.totals() {
            self.lines.insert(index, line);
            return Err(error);
        }
        Ok(RemovedLine::from(line))
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.ticket_discount = None;
        self.next_key = 0;
    }

    /// Totales sin aplicar redondeo de efectivo.
    pub fn totals(&self) -> Result<TicketTotals, CoreError> {
        self.totals_with_rounding(false)
    }

    /// Totales aplicando (o no) el redondeo de efectivo configurado.
    pub fn totals_with_rounding(&self, apply_cash_rounding: bool) -> Result<TicketTotals, CoreError> {
        let mut buckets: BTreeMap<TaxCategoryId, (Rate, Money)> = BTreeMap::new();
        let mut line_amounts = Vec::with_capacity(self.lines.len());
        let mut net_before_discounts = Money::ZERO;
        let mut line_discounts = Money::ZERO;
        let mut net_after_line_discounts = Money::ZERO;

        for line in &self.lines {
            let before = line.net_before_discount()?;
            let discount = line.discount_amount()?;
            let net = before - discount;

            net_before_discounts += before;
            line_discounts += discount;
            net_after_line_discounts += net;

            let category = line.product.tax_category_id.clone();
            let entry = buckets
                .entry(category)
                .or_insert((line.product.tax_rate, Money::ZERO));
            if entry.0 != line.product.tax_rate {
                return Err(CoreError::InvalidRate(format!(
                    "la categoría de impuesto {} tiene dos tasas distintas",
                    line.product.tax_category_id
                )));
            }
            entry.1 += net;

            line_amounts.push(LineAmount {
                key: line.key,
                net_before_discount: before,
                discount,
                net,
            });
        }

        let ticket_discount = match self.ticket_discount {
            Some(discount) => discount.amount_for(net_after_line_discounts)?,
            None => Money::ZERO,
        };

        let allocations = allocate_discount(&buckets, ticket_discount)?;
        for allocation in &allocations {
            if let Some((_, net)) = buckets.get_mut(&allocation.tax_category_id) {
                *net -= allocation.amount;
            }
        }

        let mut net = Money::ZERO;
        let mut tax = Money::ZERO;
        let mut tax_breakdown = Vec::with_capacity(buckets.len());
        for (category, (rate, base)) in &buckets {
            net += *base;
            let amount = Money::from_decimal(base.as_decimal() * rate.as_decimal());
            tax += amount;
            tax_breakdown.push(TaxAmount {
                tax_category_id: category.clone(),
                rate: *rate,
                base: *base,
                tax: amount,
            });
        }

        let total_before_rounding = net + tax;
        let (total, rounding_adjustment) = if apply_cash_rounding {
            self.cash_rounding.apply(total_before_rounding)?
        } else {
            (total_before_rounding, Money::ZERO)
        };

        Ok(TicketTotals {
            line_count: self.lines.len(),
            net_before_discounts,
            line_discounts,
            ticket_discount,
            total_discounts: line_discounts + ticket_discount,
            net,
            tax,
            total_before_rounding,
            rounding_adjustment,
            total,
            tax_breakdown,
            ticket_discount_allocations: allocations,
            line_amounts,
        })
    }

    /// Expande el ticket a las líneas que se guardan en `ticketlines`.
    pub fn sale_lines_for(&self, totals: &TicketTotals) -> Vec<SaleLine> {
        let mut result = Vec::with_capacity(self.lines.len() * 2 + 2);
        let amount_by_key: BTreeMap<LineKey, &LineAmount> =
            totals.line_amounts.iter().map(|amount| (amount.key, amount)).collect();
        let rate_by_category: BTreeMap<TaxCategoryId, Rate> = self
            .lines
            .iter()
            .map(|line| (line.product.tax_category_id.clone(), line.product.tax_rate))
            .collect();

        for line in &self.lines {
            let Some(amount) = amount_by_key.get(&line.key) else {
                continue;
            };

            let mut attributes = BTreeMap::new();
            attributes.insert("product.code".to_owned(), line.product.code.clone());
            attributes.insert("product.name".to_owned(), line.product.name.clone());
            attributes.insert(
                "product.taxcategoryid".to_owned(),
                line.product.tax_category_id.to_string(),
            );
            attributes.insert(
                "product.unit".to_owned(),
                line.product.unit.code().to_owned(),
            );
            if let Some(note) = &line.note {
                attributes.insert("pos.note".to_owned(), note.clone());
            }
            if let Some(original) = line.original_unit_price_net() {
                attributes.insert("pos.price.original_net".to_owned(), original.to_plain_string());
            }

            result.push(SaleLine {
                kind: SaleLineKind::Product,
                product_id: Some(line.product.product_id.clone()),
                code: Some(line.product.code.clone()),
                name: line.product.name.clone(),
                quantity: line.quantity,
                unit_price_net: line.product.unit_price_net,
                tax_category_id: line.product.tax_category_id.clone(),
                tax_rate: line.product.tax_rate,
                attributes,
            });

            if amount.discount.is_zero() {
                continue;
            }

            let mut discount_attributes = BTreeMap::new();
            discount_attributes.insert("pos.discount.scope".to_owned(), "line".to_owned());
            discount_attributes.insert(
                "pos.discount.source_line".to_owned(),
                line.key.to_string(),
            );
            discount_attributes.insert(
                "pos.discount.amount".to_owned(),
                amount.discount.to_plain_string(),
            );
            if let Some(rate) = line.discount.and_then(Discount::percent_value) {
                discount_attributes.insert(
                    "pos.discount.percent".to_owned(),
                    rate.as_percent().normalize().to_string(),
                );
            }

            result.push(SaleLine {
                kind: SaleLineKind::LineDiscount,
                product_id: None,
                code: None,
                name: format!("Descuento {}", line.product.name),
                quantity: Qty::one(),
                unit_price_net: -amount.discount,
                tax_category_id: line.product.tax_category_id.clone(),
                tax_rate: line.product.tax_rate,
                attributes: discount_attributes,
            });
        }

        for allocation in &totals.ticket_discount_allocations {
            if allocation.amount.is_zero() {
                continue;
            }
            let Some(rate) = rate_by_category.get(&allocation.tax_category_id).copied() else {
                continue;
            };
            let mut attributes = BTreeMap::new();
            attributes.insert("pos.discount.scope".to_owned(), "ticket".to_owned());
            attributes.insert(
                "pos.discount.amount".to_owned(),
                allocation.amount.to_plain_string(),
            );
            result.push(SaleLine {
                kind: SaleLineKind::TicketDiscount,
                product_id: None,
                code: None,
                name: "Descuento total".to_owned(),
                quantity: Qty::one(),
                unit_price_net: -allocation.amount,
                tax_category_id: allocation.tax_category_id.clone(),
                tax_rate: rate,
                attributes,
            });
        }

        if !totals.rounding_adjustment.is_zero() {
            let mut attributes = BTreeMap::new();
            attributes.insert("pos.rounding".to_owned(), "true".to_owned());
            result.push(SaleLine {
                kind: SaleLineKind::Rounding,
                product_id: None,
                code: None,
                name: "Ajuste de redondeo".to_owned(),
                quantity: Qty::one(),
                unit_price_net: totals.rounding_adjustment,
                tax_category_id: TaxCategoryId::new(TAX_CATEGORY_SIN_IVA),
                tax_rate: Rate::ZERO,
                attributes,
            });
        }

        result
    }

    /// Valida el carrito contra los pagos y devuelve lo necesario para persistir.
    pub fn checkout(&self, payments: &Payments) -> Result<Checkout, CoreError> {
        if self.lines.is_empty() {
            return Err(CoreError::EmptyCart);
        }
        let totals = self.totals_with_rounding(payments.has_cash())?;
        let payment_summary = payments.summary(totals.total)?;
        if !payment_summary.is_complete() {
            return Err(CoreError::InsufficientPayment(payment_summary.remaining));
        }
        let lines = self.sale_lines_for(&totals);
        Ok(Checkout {
            totals,
            lines,
            payments: payment_summary,
        })
    }

    fn index_of(&self, key: LineKey) -> Result<usize, CoreError> {
        self.lines
            .iter()
            .position(|line| line.key == key)
            .ok_or(CoreError::LineNotFound(key))
    }

    fn validate_quantity(&self, unit: crate::quantity::SaleUnit, quantity: Qty) -> Result<(), CoreError> {
        if !quantity.is_positive() {
            return Err(CoreError::NonPositiveQuantity);
        }
        if !unit.is_weighted() && !quantity.is_integer() {
            return Err(CoreError::FractionalUnitQuantity);
        }
        Ok(())
    }
}

fn allocate_discount(
    buckets: &BTreeMap<TaxCategoryId, (Rate, Money)>,
    discount: Money,
) -> Result<Vec<DiscountAllocation>, CoreError> {
    if discount.is_zero() {
        return Ok(Vec::new());
    }
    let total = buckets.values().map(|(_, net)| *net).sum::<Money>();
    if total.is_zero() {
        return Err(CoreError::DiscountExceedsBase);
    }
    if discount > total {
        return Err(CoreError::DiscountExceedsBase);
    }

    let total_cents = total.to_cents();
    let discount_cents = discount.to_cents();
    let mut rows: Vec<(TaxCategoryId, i64, i64)> = buckets
        .iter()
        .map(|(category, (_, net))| {
            let net_cents = net.to_cents();
            let product = net_cents * discount_cents;
            let floor = product / total_cents;
            let remainder = product % total_cents;
            (category.clone(), floor, remainder)
        })
        .collect();

    let allocated: i64 = rows.iter().map(|(_, floor, _)| *floor).sum();
    let mut remaining = discount_cents - allocated;
    rows.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));

    let mut result = Vec::with_capacity(rows.len());
    for (category, mut cents, _) in rows {
        if remaining > 0 {
            cents += 1;
            remaining -= 1;
        }
        result.push(DiscountAllocation {
            tax_category_id: category,
            amount: Money::from_cents(cents),
        });
    }
    result.sort_by(|a, b| a.tax_category_id.cmp(&b.tax_category_id));
    Ok(result)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovedLine {
    pub key: LineKey,
    pub product_id: ProductId,
    pub code: String,
    pub name: String,
    pub quantity: Qty,
    pub unit_price_net: Money,
    pub note: Option<String>,
}

impl From<TicketLine> for RemovedLine {
    fn from(line: TicketLine) -> Self {
        Self {
            key: line.key,
            product_id: line.product.product_id,
            code: line.product.code,
            name: line.product.name,
            quantity: line.quantity,
            unit_price_net: line.product.unit_price_net,
            note: line.note,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaleLineKind {
    Product,
    LineDiscount,
    TicketDiscount,
    Rounding,
}

impl SaleLineKind {
    pub fn is_product(self) -> bool {
        matches!(self, Self::Product)
    }
}

/// Línea tal como se guardará en `ticketlines`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaleLine {
    pub kind: SaleLineKind,
    pub product_id: Option<ProductId>,
    pub code: Option<String>,
    pub name: String,
    pub quantity: Qty,
    pub unit_price_net: Money,
    pub tax_category_id: TaxCategoryId,
    pub tax_rate: Rate,
    pub attributes: BTreeMap<String, String>,
}

impl SaleLine {
    pub fn net_total(&self) -> Money {
        self.unit_price_net * self.quantity
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkout {
    pub totals: TicketTotals,
    pub lines: Vec<SaleLine>,
    pub payments: PaymentSummary,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{ProductId, TaxCategoryId};
    use crate::model::Product;
    use crate::payment::{Payment, PaymentMethod};
    use crate::quantity::{Qty, SaleUnit};
    use rust_decimal_macros::dec;

    fn product(id: &str, price: i64, tax_percent: &str, unit: SaleUnit) -> Product {
        Product {
            id: ProductId::new(id),
            code: format!("CODE-{id}"),
            reference: format!("REF-{id}"),
            name: format!("Producto {id}"),
            category_id: None,
            tax_category_id: TaxCategoryId::new("100"),
            tax_rate: Rate::parse_percent(tax_percent).unwrap(),
            list_price: Money::from_pesos(price),
            cost: Money::from_pesos(0),
            unit,
            active: true,
            stock: None,
        }
    }

    fn add(draft: &mut TicketDraft, product: &Product, qty: Qty) -> LineKey {
        draft
            .add_product(product, qty, PriceMode::TaxExcluded)
            .unwrap()
    }

    #[test]
    fn agrega_y_calcula_total_sin_impuesto() {
        let mut draft = TicketDraft::new();
        add(&mut draft, &product("p1", 4_600, "0", SaleUnit::Unit), Qty::from_units(2));
        let totals = draft.totals().unwrap();
        assert_eq!(totals.line_count, 1);
        assert_eq!(totals.net, Money::from_pesos(9_200));
        assert_eq!(totals.tax, Money::ZERO);
        assert_eq!(totals.total, Money::from_pesos(9_200));
    }

    #[test]
    fn escanear_el_mismo_producto_fusiona_la_linea() {
        let mut draft = TicketDraft::new();
        let p = product("p1", 4_600, "0", SaleUnit::Unit);
        let key1 = add(&mut draft, &p, Qty::one());
        let key2 = add(&mut draft, &p, Qty::one());
        assert_eq!(key1, key2);
        assert_eq!(draft.line_count(), 1);
        assert_eq!(draft.lines()[0].quantity(), Qty::from_units(2));
    }

    #[test]
    fn precio_editado_recalcula_iva_y_no_se_mezcla_con_escaneos_nuevos() {
        let mut draft = TicketDraft::new();
        let p = product("p1", 10_000, "19", SaleUnit::Unit);
        let key = add(&mut draft, &p, Qty::one());
        draft.set_line_discount(key, Some(Discount::amount(Money::from_pesos(5_000)))).unwrap();
        assert_eq!(draft.edit_line(key, Qty::from_units(2), Some(Money::from_pesos(1_000))),
            Err(CoreError::DiscountExceedsBase));
        assert_eq!(draft.lines()[0].quantity(), Qty::one());
        assert_eq!(draft.lines()[0].original_unit_price_net(), None);
        draft.edit_line(key, Qty::from_units(2), Some(Money::from_pesos(7_500))).unwrap();
        assert_eq!(draft.totals().unwrap().total, Money::from_pesos(11_900));
        assert_eq!(draft.lines()[0].original_unit_price_net(), Some(Money::from_pesos(10_000)));
        assert_ne!(add(&mut draft, &p, Qty::one()), key);
        assert_eq!(draft.totals().unwrap().total, Money::from_pesos(23_800));
    }

    #[test]
    fn producto_pesable_usa_tres_decimales() {
        let mut draft = TicketDraft::new();
        let p = product("p1", 4_600, "0", SaleUnit::Kilogram);
        add(&mut draft, &p, Qty::from_grams(850));
        let totals = draft.totals().unwrap();
        assert_eq!(totals.total, Money::from_pesos(3_910));
        assert_eq!(draft.lines()[0].quantity().format(SaleUnit::Kilogram), "0,850 kg");
    }

    #[test]
    fn no_permite_fracciones_en_unidades() {
        let mut draft = TicketDraft::new();
        let p = product("p1", 4_600, "0", SaleUnit::Unit);
        let error = draft
            .add_product(&p, Qty::from_decimal(dec!(1.5)), PriceMode::TaxExcluded)
            .unwrap_err();
        assert_eq!(error, CoreError::FractionalUnitQuantity);
    }

    #[test]
    fn descuento_por_linea_porcentaje() {
        let mut draft = TicketDraft::new();
        let p = product("p1", 10_000, "0", SaleUnit::Unit);
        let key = add(&mut draft, &p, Qty::one());
        draft
            .set_line_discount(key, Some(Discount::percent(Rate::parse_percent("10").unwrap())))
            .unwrap();
        let totals = draft.totals().unwrap();
        assert_eq!(totals.line_discounts, Money::from_pesos(1_000));
        assert_eq!(totals.total, Money::from_pesos(9_000));
    }

    #[test]
    fn descuento_total_porcentaje() {
        let mut draft = TicketDraft::new();
        let p1 = product("p1", 10_000, "0", SaleUnit::Unit);
        let p2 = product("p2", 5_000, "0", SaleUnit::Unit);
        add(&mut draft, &p1, Qty::one());
        add(&mut draft, &p2, Qty::one());
        draft
            .set_ticket_discount(Some(Discount::percent(
                Rate::parse_percent("10").unwrap(),
            )))
            .unwrap();
        let totals = draft.totals().unwrap();
        assert_eq!(totals.ticket_discount, Money::from_pesos(1_500));
        assert_eq!(totals.total, Money::from_pesos(13_500));
    }

    #[test]
    fn precio_con_iva_incluido_se_descompone_en_linea() {
        let mut draft = TicketDraft::new();
        let p = product("p1", 11_900, "19", SaleUnit::Unit);
        draft
            .add_product(&p, Qty::from_units(2), PriceMode::TaxIncluded)
            .unwrap();
        let totals = draft.totals().unwrap();
        assert_eq!(totals.net, Money::from_pesos(20_000));
        assert_eq!(totals.tax, Money::from_pesos(3_800));
        assert_eq!(totals.total, Money::from_pesos(23_800));
        assert_eq!(totals.tax_breakdown.len(), 1);
        assert_eq!(totals.tax_breakdown[0].rate.as_decimal(), dec!(0.19));
    }

    #[test]
    fn iva_al_cinco_por_ciento() {
        let mut draft = TicketDraft::new();
        let p = product("p1", 10_500, "5", SaleUnit::Unit);
        draft
            .add_product(&p, Qty::one(), PriceMode::TaxIncluded)
            .unwrap();
        let totals = draft.totals().unwrap();
        assert_eq!(totals.net, Money::from_pesos(10_000));
        assert_eq!(totals.tax, Money::from_pesos(500));
        assert_eq!(totals.total, Money::from_pesos(10_500));
    }

    #[test]
    fn descuento_total_se_reparte_por_categoria_de_impuesto() {
        let mut draft = TicketDraft::new();
        let p1 = product("p1", 10_000, "0", SaleUnit::Unit);
        let mut p2 = product("p2", 11_900, "19", SaleUnit::Unit);
        p2.tax_category_id = TaxCategoryId::new("102");
        draft
            .add_product(&p1, Qty::one(), PriceMode::TaxIncluded)
            .unwrap();
        draft
            .add_product(&p2, Qty::one(), PriceMode::TaxIncluded)
            .unwrap();
        draft
            .set_ticket_discount(Some(Discount::percent(
                Rate::parse_percent("10").unwrap(),
            )))
            .unwrap();
        let totals = draft.totals().unwrap();
        assert_eq!(totals.ticket_discount, Money::from_pesos(2_000));
        assert_eq!(
            totals
                .ticket_discount_allocations
                .iter()
                .map(|a| a.amount)
                .sum::<Money>(),
            Money::from_pesos(2_000)
        );
        assert_eq!(totals.net, Money::from_pesos(18_000));
        assert_eq!(totals.tax, Money::from_pesos(1_710));
        assert_eq!(totals.total, Money::from_pesos(19_710));
    }

    #[test]
    fn redondeo_de_efectivo_se_aplica_solo_al_cobrar() {
        let mut draft = TicketDraft::new();
        let p = product("p1", 9_233, "0", SaleUnit::Unit);
        add(&mut draft, &p, Qty::one());
        draft.set_cash_rounding(CashRounding::Nearest50);

        let without_rounding = draft.totals().unwrap();
        assert_eq!(without_rounding.total, Money::from_pesos(9_233));
        assert_eq!(without_rounding.rounding_adjustment, Money::ZERO);

        let with_rounding = draft.totals_with_rounding(true).unwrap();
        assert_eq!(with_rounding.total, Money::from_pesos(9_250));
        assert_eq!(with_rounding.rounding_adjustment, Money::from_pesos(17));
    }

    #[test]
    fn expande_descuentos_y_redondeo_a_lineas_persistibles() {
        let mut draft = TicketDraft::new();
        let p = product("p1", 10_000, "0", SaleUnit::Unit);
        let key = add(&mut draft, &p, Qty::one());
        draft
            .set_line_discount(key, Some(Discount::percent(Rate::parse_percent("10").unwrap())))
            .unwrap();
        draft
            .set_ticket_discount(Some(Discount::percent(
                Rate::parse_percent("10").unwrap(),
            )))
            .unwrap();
        let totals = draft.totals().unwrap();
        let lines = draft.sale_lines_for(&totals);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].kind, SaleLineKind::Product);
        assert_eq!(lines[1].kind, SaleLineKind::LineDiscount);
        assert_eq!(lines[1].unit_price_net, Money::from_pesos(-1_000));
        assert_eq!(lines[2].kind, SaleLineKind::TicketDiscount);
        assert_eq!(lines[2].unit_price_net, Money::from_pesos(-900));
        assert_eq!(lines.iter().map(SaleLine::net_total).sum::<Money>(), Money::from_pesos(8_100));
    }

    #[test]
    fn genera_linea_de_ajuste_de_redondeo() {
        let mut draft = TicketDraft::new();
        let p = product("p1", 9_233, "0", SaleUnit::Unit);
        add(&mut draft, &p, Qty::one());
        draft.set_cash_rounding(CashRounding::Nearest50);
        let totals = draft.totals_with_rounding(true).unwrap();
        let lines = draft.sale_lines_for(&totals);
        assert_eq!(lines.last().unwrap().kind, SaleLineKind::Rounding);
        assert_eq!(lines.last().unwrap().name, "Ajuste de redondeo");
        assert_eq!(lines.last().unwrap().unit_price_net, Money::from_pesos(17));
    }

    #[test]
    fn la_nota_se_guarda_en_atributos_y_se_imprime() {
        let mut draft = TicketDraft::new();
        let p = product("p1", 10_000, "0", SaleUnit::Unit);
        let key = add(&mut draft, &p, Qty::one());
        draft.set_note(key, Some("sin bolsa")).unwrap();
        let totals = draft.totals().unwrap();
        let lines = draft.sale_lines_for(&totals);
        assert_eq!(lines[0].attributes.get("pos.note").unwrap(), "sin bolsa");
        assert_eq!(draft.lines()[0].note(), Some("sin bolsa"));
    }

    #[test]
    fn eliminar_linea_devuelve_lo_necesario_para_lineremoved() {
        let mut draft = TicketDraft::new();
        let p = product("p1", 4_600, "0", SaleUnit::Kilogram);
        let key = add(&mut draft, &p, Qty::from_grams(1_535));
        let removed = draft.remove_line(key).unwrap();
        assert_eq!(removed.product_id, ProductId::new("p1"));
        assert_eq!(removed.name, "Producto p1");
        assert_eq!(removed.quantity, Qty::from_grams(1_535));
        assert!(draft.is_empty());
    }

    #[test]
    fn no_permite_eliminar_si_deja_descuento_total_invalido() {
        let mut draft = TicketDraft::new();
        let p = product("p1", 10_000, "0", SaleUnit::Unit);
        let key = add(&mut draft, &p, Qty::one());
        draft
            .set_ticket_discount(Some(Discount::amount(Money::from_pesos(10_000))))
            .unwrap();
        assert_eq!(
            draft.remove_line(key),
            Err(CoreError::DiscountExceedsBase)
        );
        assert_eq!(draft.line_count(), 1);
    }

    #[test]
    fn descuento_de_importe_no_puede_superar_la_linea() {
        let mut draft = TicketDraft::new();
        let p = product("p1", 10_000, "0", SaleUnit::Unit);
        let key = add(&mut draft, &p, Qty::one());
        assert_eq!(
            draft.set_line_discount(key, Some(Discount::amount(Money::from_pesos(10_001)))),
            Err(CoreError::DiscountExceedsBase)
        );
    }

    #[test]
    fn nota_demasiado_larga() {
        let mut draft = TicketDraft::new();
        let p = product("p1", 10_000, "0", SaleUnit::Unit);
        let key = add(&mut draft, &p, Qty::one());
        let note = "x".repeat(81);
        assert!(matches!(
            draft.set_note(key, Some(&note)),
            Err(CoreError::TextTooLong { max: 80 })
        ));
    }

    #[test]
    fn checkout_valida_carrito_y_pagos() {
        let mut draft = TicketDraft::new();
        let p = product("p1", 10_000, "0", SaleUnit::Unit);
        add(&mut draft, &p, Qty::one());

        let mut payments = Payments::new();
        payments.add(Payment::cash(Money::from_pesos(10_000), Money::from_pesos(10_000)).unwrap());
        let checkout = draft.checkout(&payments).unwrap();
        assert_eq!(checkout.totals.total, Money::from_pesos(10_000));
        assert_eq!(checkout.lines.len(), 1);
        assert_eq!(checkout.payments.change, Money::ZERO);

        let empty = TicketDraft::new();
        assert_eq!(empty.checkout(&payments), Err(CoreError::EmptyCart));
    }

    #[test]
    fn checkout_aplica_redondeo_si_hay_efectivo() {
        let mut draft = TicketDraft::new();
        let p = product("p1", 9_233, "0", SaleUnit::Unit);
        add(&mut draft, &p, Qty::one());
        draft.set_cash_rounding(CashRounding::Nearest50);

        let mut cash = Payments::new();
        cash.add(Payment::cash(Money::from_pesos(9_250), Money::from_pesos(10_000)).unwrap());
        let checkout = draft.checkout(&cash).unwrap();
        assert_eq!(checkout.totals.total, Money::from_pesos(9_250));
        assert_eq!(checkout.payments.change, Money::from_pesos(750));

        let mut card = Payments::new();
        card.add(Payment::electronic(PaymentMethod::Magcard, Money::from_pesos(9_233), None).unwrap());
        let checkout = draft.checkout(&card).unwrap();
        assert_eq!(checkout.totals.total, Money::from_pesos(9_233));
        assert_eq!(checkout.totals.rounding_adjustment, Money::ZERO);
    }

    #[test]
    fn total_negativo_no_es_posible_por_descuentos_validados() {
        let mut draft = TicketDraft::new();
        let p = product("p1", 10_000, "0", SaleUnit::Unit);
        let key = add(&mut draft, &p, Qty::one());
        assert!(draft
            .set_line_discount(key, Some(Discount::amount(Money::from_pesos(10_000))))
            .is_ok());
        let totals = draft.totals().unwrap();
        assert_eq!(totals.total, Money::ZERO);
    }
}
