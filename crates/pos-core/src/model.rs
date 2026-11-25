use crate::ids::{CategoryId, LocationId, ProductId, TaxCategoryId, UserId};
use crate::money::Money;
use crate::permissions::RoleKind;
use crate::quantity::{Qty, SaleUnit};
use crate::rate::Rate;

/// Cómo se interpreta el precio de catálogo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PriceMode {
    /// El precio de catálogo ya incluye impuesto.
    TaxIncluded,
    /// El precio de catálogo es neto y el impuesto se suma.
    TaxExcluded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Category {
    pub id: CategoryId,
    pub name: String,
    pub parent_id: Option<CategoryId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaxCategory {
    pub id: TaxCategoryId,
    pub name: String,
    pub rate: Rate,
}

/// Producto tal como lo necesita el dominio de venta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Product {
    pub id: ProductId,
    pub code: String,
    pub reference: String,
    pub name: String,
    pub category_id: Option<CategoryId>,
    pub tax_category_id: TaxCategoryId,
    pub tax_rate: Rate,
    /// Precio de catálogo, en el modo configurado (normalmente con impuesto).
    pub list_price: Money,
    pub cost: Money,
    pub unit: SaleUnit,
    pub active: bool,
    pub stock: Option<Qty>,
}

impl Product {
    /// Precio unitario neto (sin impuesto) tal como se guarda en `ticketlines.PRICE`.
    pub fn unit_price_net(&self, mode: PriceMode) -> Money {
        match mode {
            PriceMode::TaxExcluded => self.list_price,
            PriceMode::TaxIncluded => {
                if self.tax_rate.is_zero() {
                    self.list_price
                } else {
                    Money::from_decimal(
                        self.list_price.as_decimal() / self.tax_rate.multiplier(),
                    )
                }
            }
        }
    }

    /// Copia inmutable de lo que necesita una línea de ticket.
    pub fn snapshot(&self, mode: PriceMode) -> ProductSnapshot {
        ProductSnapshot {
            product_id: self.id.clone(),
            code: self.code.clone(),
            name: self.name.clone(),
            unit: self.unit,
            tax_category_id: self.tax_category_id.clone(),
            tax_rate: self.tax_rate,
            unit_price_net: self.unit_price_net(mode),
        }
    }
}

/// Datos congelados de un producto al agregarlo al ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductSnapshot {
    pub product_id: ProductId,
    pub code: String,
    pub name: String,
    pub unit: SaleUnit,
    pub tax_category_id: TaxCategoryId,
    pub tax_rate: Rate,
    pub unit_price_net: Money,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Role {
    pub id: String,
    pub name: String,
    pub kind: RoleKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub id: UserId,
    pub name: String,
    pub role: RoleKind,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub id: LocationId,
    pub name: String,
}

/// Constante útil para los productos históricos “Sin IVA”.
pub const TAX_CATEGORY_SIN_IVA: &str = "100";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::TaxCategoryId;
    use rust_decimal_macros::dec;

    fn product(price: i64, percent: &str) -> Product {
        Product {
            id: ProductId::new("p1"),
            code: "001".into(),
            reference: "REF".into(),
            name: "Producto".into(),
            category_id: None,
            tax_category_id: TaxCategoryId::new("102"),
            tax_rate: Rate::parse_percent(percent).unwrap(),
            list_price: Money::from_pesos(price),
            cost: Money::from_pesos(0),
            unit: SaleUnit::Unit,
            active: true,
            stock: None,
        }
    }

    #[test]
    fn sin_iva_el_neto_es_el_precio() {
        let p = product(4_600, "0");
        assert_eq!(p.unit_price_net(PriceMode::TaxIncluded), Money::from_pesos(4_600));
    }

    #[test]
    fn precio_con_iva_incluido_se_descompone() {
        let p = product(11_900, "19");
        assert_eq!(p.unit_price_net(PriceMode::TaxIncluded), Money::from_pesos(10_000));
        assert_eq!(p.unit_price_net(PriceMode::TaxExcluded), Money::from_pesos(11_900));
    }

    #[test]
    fn snapshot_congela_los_datos() {
        let p = product(11_900, "19");
        let s = p.snapshot(PriceMode::TaxIncluded);
        assert_eq!(s.unit_price_net, Money::from_pesos(10_000));
        assert_eq!(s.tax_rate.as_decimal(), dec!(0.19));
        assert_eq!(s.code, "001");
    }
}
