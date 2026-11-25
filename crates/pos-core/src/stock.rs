use crate::error::CoreError;
use crate::ids::{LocationId, ProductId, UserId};
use crate::money::Money;
use crate::quantity::Qty;
use crate::ticket::{Checkout, SaleLine, SaleLineKind};

/// Motivo del movimiento de inventario, alineado con `stockdiary.REASON`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StockReason {
    Sale,
    Entry,
}

impl StockReason {
    pub const fn as_i32(self) -> i32 {
        match self {
            Self::Sale => -1,
            Self::Entry => 1,
        }
    }
}

/// Movimiento que el store escribirá en `stockdiary` y aplicará a `stockcurrent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StockMovement {
    pub product_id: ProductId,
    pub location_id: LocationId,
    /// Negativo para ventas, positivo para entradas.
    pub quantity_delta: Qty,
    pub unit_cost: Money,
    pub reason: StockReason,
    pub user_id: UserId,
    pub reference: Option<String>,
}

impl StockMovement {
    pub fn new(
        product_id: ProductId,
        location_id: LocationId,
        quantity_delta: Qty,
        unit_cost: Money,
        reason: StockReason,
        user_id: UserId,
        reference: Option<String>,
    ) -> Result<Self, CoreError> {
        if quantity_delta.is_zero() {
            return Err(CoreError::InvalidStockMovement(
                "la cantidad no puede ser cero".to_owned(),
            ));
        }
        if unit_cost.is_negative() {
            return Err(CoreError::InvalidStockMovement(
                "el costo/precio unitario no puede ser negativo".to_owned(),
            ));
        }
        match reason {
            StockReason::Sale if !quantity_delta.is_negative() => {
                return Err(CoreError::InvalidStockMovement(
                    "una venta debe restar stock".to_owned(),
                ));
            }
            StockReason::Entry if !quantity_delta.is_positive() => {
                return Err(CoreError::InvalidStockMovement(
                    "una entrada debe sumar stock".to_owned(),
                ));
            }
            _ => {}
        }
        let reference = reference.and_then(|value| {
            let trimmed = value.trim().to_owned();
            (!trimmed.is_empty()).then_some(trimmed)
        });
        Ok(Self {
            product_id,
            location_id,
            quantity_delta,
            unit_cost,
            reason,
            user_id,
            reference,
        })
    }

    pub fn sale(
        product_id: ProductId,
        location_id: LocationId,
        quantity: Qty,
        unit_price_net: Money,
        user_id: UserId,
        ticket_reference: Option<String>,
    ) -> Result<Self, CoreError> {
        if !quantity.is_positive() {
            return Err(CoreError::InvalidStockMovement(
                "la cantidad vendida debe ser positiva".to_owned(),
            ));
        }
        Self::new(
            product_id,
            location_id,
            -quantity,
            unit_price_net,
            StockReason::Sale,
            user_id,
            ticket_reference,
        )
    }

    pub fn entry(
        product_id: ProductId,
        location_id: LocationId,
        quantity: Qty,
        unit_cost: Money,
        user_id: UserId,
        reference: Option<String>,
    ) -> Result<Self, CoreError> {
        Self::new(
            product_id,
            location_id,
            quantity,
            unit_cost,
            StockReason::Entry,
            user_id,
            reference,
        )
    }
}

/// Movimientos de stock de una venta ya calculada.
pub fn sale_stock_movements(
    checkout: &Checkout,
    location_id: &LocationId,
    user_id: &UserId,
    ticket_reference: Option<String>,
) -> Result<Vec<StockMovement>, CoreError> {
    let mut movements = Vec::new();
    for line in &checkout.lines {
        if !line.kind.is_product() {
            continue;
        }
        let Some(product_id) = &line.product_id else {
            continue;
        };
        movements.push(StockMovement::sale(
            product_id.clone(),
            location_id.clone(),
            line.quantity,
            line.unit_price_net,
            user_id.clone(),
            ticket_reference.clone(),
        )?);
    }
    Ok(movements)
}

/// Aplica un delta a un stock actual. Los negativos son válidos y se conservan.
pub fn apply_delta(current: Option<Qty>, delta: Qty) -> Qty {
    current.unwrap_or(Qty::ZERO) + delta
}

/// Comprueba que una línea persistible es de producto, para no restar stock por
/// descuentos o ajustes de redondeo.
pub fn is_stock_line(line: &SaleLine) -> bool {
    line.kind == SaleLineKind::Product && line.product_id.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cash::CashRounding;
    use crate::ids::{CategoryId, TaxCategoryId};
    use crate::model::{PriceMode, Product};
    use crate::payment::{Payment, Payments};
    use crate::quantity::SaleUnit;
    use crate::rate::Rate;
    use crate::ticket::Discount;
    use crate::ticket::TicketDraft;

    fn product(id: &str, price: i64) -> Product {
        Product {
            id: ProductId::new(id),
            code: format!("C-{id}"),
            reference: format!("R-{id}"),
            name: format!("P-{id}"),
            category_id: None::<CategoryId>,
            tax_category_id: TaxCategoryId::new("100"),
            tax_rate: Rate::ZERO,
            list_price: Money::from_pesos(price),
            cost: Money::from_pesos(0),
            unit: SaleUnit::Unit,
            active: true,
            stock: None,
        }
    }

    fn checkout_with_discount() -> Checkout {
        let mut draft = TicketDraft::new();
        let p = product("p1", 10_000);
        let key = draft
            .add_product(&p, Qty::one(), PriceMode::TaxExcluded)
            .unwrap();
        draft
            .set_line_discount(key, Some(Discount::percent(Rate::parse_percent("10").unwrap())))
            .unwrap();
        draft.set_cash_rounding(CashRounding::Nearest50);
        let mut payments = Payments::new();
        payments.add(
            Payment::cash(Money::from_pesos(9_000), Money::from_pesos(10_000)).unwrap(),
        );
        draft.checkout(&payments).unwrap()
    }

    #[test]
    fn venta_resta_stock_y_solo_incluye_lineas_de_producto() {
        let checkout = checkout_with_discount();
        let movements = sale_stock_movements(
            &checkout,
            &LocationId::new("0"),
            &UserId::new("0"),
            Some("23006".to_owned()),
        )
        .unwrap();
        assert_eq!(movements.len(), 1);
        assert_eq!(movements[0].quantity_delta, Qty::from_units(-1));
        assert_eq!(movements[0].reason, StockReason::Sale);
        assert_eq!(movements[0].reason.as_i32(), -1);
        assert_eq!(movements[0].reference.as_deref(), Some("23006"));
    }

    #[test]
    fn stock_negativo_es_valido() {
        let current = Qty::from_units(-2);
        let result = apply_delta(Some(current), Qty::from_units(-1));
        assert_eq!(result, Qty::from_units(-3));
    }

    #[test]
    fn entrada_suma_stock() {
        let movement = StockMovement::entry(
            ProductId::new("p1"),
            LocationId::new("0"),
            Qty::from_units(10),
            Money::from_pesos(3_000),
            UserId::new("0"),
            Some("FAC-1".to_owned()),
        )
        .unwrap();
        assert_eq!(movement.reason, StockReason::Entry);
        assert_eq!(movement.reason.as_i32(), 1);
        assert_eq!(
            apply_delta(Some(Qty::from_units(-2)), movement.quantity_delta),
            Qty::from_units(8)
        );
    }

    #[test]
    fn rechaza_movimientos_incoherentes() {
        assert!(StockMovement::sale(
            ProductId::new("p1"),
            LocationId::new("0"),
            Qty::from_units(1),
            Money::from_pesos(100),
            UserId::new("0"),
            None,
        )
        .is_ok());

        assert!(StockMovement::new(
            ProductId::new("p1"),
            LocationId::new("0"),
            Qty::ZERO,
            Money::ZERO,
            StockReason::Sale,
            UserId::new("0"),
            None,
        )
        .is_err());
    }
}
