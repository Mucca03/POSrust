//! Carga del catálogo operativo desde SQLite.
//!
//! Solo se cargan productos activos y la bodega principal. Los importes se
//! convierten a `Decimal` una vez, al arrancar; la UI no vuelve a tocar texto
//! decimal ni `f64`.

use std::collections::BTreeMap;
use std::str::FromStr;

use pos_core::{
    Category, CategoryId, Location, LocationId, Money, PriceMode, Product, ProductId, Qty, Rate,
    RoleKind, SaleUnit, TaxCategory, TaxCategoryId, User, UserId,
};
use rusqlite::Connection;
use rust_decimal::Decimal;

use crate::error::StoreError;

#[derive(Debug, Clone)]
pub struct Catalog {
    pub products: Vec<Product>,
    pub categories: Vec<Category>,
    pub taxes: Vec<TaxCategory>,
    pub users: Vec<User>,
    pub location: Location,
    pub price_mode: PriceMode,
    by_code: BTreeMap<String, usize>,
    by_id: BTreeMap<String, usize>,
    search_terms: Vec<(String, String, String)>,
    grid: Vec<usize>,
}

impl Catalog {
    pub fn product_by_code(&self, code: &str) -> Option<&Product> {
        self.by_code
            .get(&normalize_code(code))
            .and_then(|index| self.products.get(*index))
    }

    pub fn product_by_id(&self, id: &str) -> Option<&Product> {
        self.by_id
            .get(id)
            .and_then(|index| self.products.get(*index))
    }

    /// Búsqueda por código, referencia o nombre; sin acentos ni mayúsculas.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&Product> {
        self.search_indices(query, limit).into_iter().map(|i| &self.products[i]).collect()
    }

    fn search_indices(&self, query: &str, limit: usize) -> Vec<usize> {
        let needle = normalize_text(query);
        if needle.is_empty() {
            return Vec::new();
        }
        let mut exact = Vec::new();
        let mut rest = Vec::new();
        for (index, (code, reference, name)) in self.search_terms.iter().enumerate() {
            if code == &needle || reference == &needle {
                exact.push(index);
            } else if code.contains(&needle)
                || reference.contains(&needle)
                || name.contains(&needle)
            {
                rest.push(index);
            }
        }
        exact.extend(rest);
        exact.truncate(limit);
        exact
    }

    /// La grilla respeta products_cat/CATORDER. Buscar sigue incluyendo todo
    /// el catálogo activo, aunque algún producto no esté en la grilla.
    pub fn visible_indices(&self, query: &str, category: Option<&str>) -> Vec<usize> {
        if !query.trim().is_empty() {
            return self.search_indices(query, self.products.len());
        }
        self.grid.iter().copied().filter(|&i| {
            category.is_none() || self.products[i].category_id.as_ref().map(|id| id.as_str()) == category
        }).collect()
    }

    pub fn products_in_category(&self, category: Option<&str>) -> Vec<&Product> {
        match category {
            None => self.products.iter().collect(),
            Some(id) => self
                .products
                .iter()
                .filter(|p| p.category_id.as_ref().map(|c| c.as_str()) == Some(id))
                .collect(),
        }
    }

    pub fn user(&self, id: &str) -> Option<&User> {
        self.users.iter().find(|u| u.id.as_str() == id)
    }

    pub fn tax(&self, id: &str) -> Option<&TaxCategory> {
        self.taxes.iter().find(|t| t.id.as_str() == id)
    }

    pub fn in_grid(&self, id: &ProductId) -> bool {
        self.by_id.get(id.as_str()).is_some_and(|index| self.grid.contains(index))
    }
}

pub fn load_catalog(conn: &Connection, price_mode: PriceMode) -> Result<Catalog, StoreError> {
    let (location_id, location_name) = conn.query_row(
        "SELECT ID, NAME FROM locations ORDER BY ID LIMIT 1",
        [],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    )?;
    let location = Location {
        id: LocationId::new(location_id),
        name: location_name,
    };

    let mut taxes = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT c.ID, c.NAME, COALESCE(t.RATE, '0')
             FROM taxcategories c
             LEFT JOIN taxes t ON t.CATEGORY = c.ID
             ORDER BY c.ID",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (id, name, rate) = row?;
            taxes.push(TaxCategory {
                id: TaxCategoryId::new(id),
                name,
                rate: Rate::from_fraction(parse_decimal(&rate, "taxes.RATE")?)?,
            });
        }
    }

    let mut categories = Vec::new();
    {
        let mut stmt =
            conn.prepare("SELECT ID, NAME, PARENTID FROM categories ORDER BY NAME")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?;
        for row in rows {
            let (id, name, parent) = row?;
            categories.push(Category {
                id: CategoryId::new(id),
                name,
                parent_id: parent.map(CategoryId::new),
            });
        }
    }

    let mut products = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT p.ID, p.CODE, p.REFERENCE, p.NAME, p.CATEGORY, p.TAXCAT,
                    p.PRICESELL, p.PRICEBUY, p.ISSCALE,
                    COALESCE(t.RATE, '0'), s.UNITS
             FROM products p
             LEFT JOIN taxes t ON t.CATEGORY = p.TAXCAT
             LEFT JOIN stockcurrent s
                    ON s.PRODUCT = p.ID
                   AND s.LOCATION = ?1
                   AND s.ATTRIBUTESETINSTANCE_ID IS NULL
             LEFT JOIN pos_product_flags f ON f.PRODUCT_ID = p.ID
             WHERE COALESCE(f.ACTIVE, 1) = 1
             ORDER BY p.NAME",
        )?;
        let rows = stmt.query_map([location.id.as_str()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, String>(9)?,
                r.get::<_, Option<String>>(10)?,
            ))
        })?;
        for row in rows {
            let (
                id,
                code,
                reference,
                name,
                category,
                taxcat,
                price,
                cost,
                is_scale,
                rate,
                stock,
            ) = row?;
            products.push(Product {
                id: ProductId::new(id),
                code,
                reference: reference.unwrap_or_default(),
                name,
                category_id: category.map(CategoryId::new),
                tax_category_id: TaxCategoryId::new(taxcat),
                tax_rate: Rate::from_fraction(parse_decimal(&rate, "taxes.RATE")?)?,
                list_price: Money::from_decimal(parse_decimal(&price, "products.PRICESELL")?),
                cost: Money::from_decimal(parse_decimal(&cost, "products.PRICEBUY")?),
                unit: if is_scale == 1 {
                    SaleUnit::Kilogram
                } else {
                    SaleUnit::Unit
                },
                active: true,
                stock: match stock {
                    Some(value) => Some(Qty::from_decimal(parse_decimal(
                        &value,
                        "stockcurrent.UNITS",
                    )?)),
                    None => None,
                },
            });
        }
    }

    let mut users = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT p.ID, p.NAME, p.ROLE
             FROM people p
             WHERE COALESCE(p.VISIBLE, 1) = 1
             ORDER BY p.NAME",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (id, name, role) = row?;
            users.push(User {
                id: UserId::new(id.clone()),
                name,
                role: RoleKind::from_legacy_id(&role).unwrap_or(RoleKind::Guest),
                active: true,
            });
        }
    }

    let mut by_code = BTreeMap::new();
    let mut by_id = BTreeMap::new();
    let mut search_terms = Vec::with_capacity(products.len());
    for (index, product) in products.iter().enumerate() {
        let code = normalize_code(&product.code);
        if !code.is_empty() {
            by_code.entry(code).or_insert(index);
        }
        by_id.insert(product.id.as_str().to_owned(), index);
        search_terms.push((
            normalize_text(&product.code), normalize_text(&product.reference), normalize_text(&product.name),
        ));
    }
    let mut grid = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT p.ID FROM products_cat g JOIN products p ON p.ID = g.PRODUCT
             ORDER BY g.CATORDER IS NULL, g.CATORDER, p.NAME, p.ID",
        )?;
        for id in stmt.query_map([], |row| row.get::<_, String>(0))? {
            if let Some(index) = by_id.get(&id?) {
                grid.push(*index);
            }
        }
    }

    Ok(Catalog {
        products,
        categories,
        taxes,
        users,
        location,
        price_mode,
        by_code,
        by_id,
        search_terms,
        grid,
    })
}

pub(crate) fn parse_decimal(value: &str, column: &str) -> Result<Decimal, StoreError> {
    Decimal::from_str(value.trim()).map_err(|e| {
        StoreError::Verification(format!("decimal inválido en {column}: {value:?} ({e})"))
    })
}

pub fn normalize_code(value: &str) -> String {
    value.trim().to_lowercase()
}

pub fn normalize_text(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            _ => c,
        })
        .collect()
}
