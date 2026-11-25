#![forbid(unsafe_code)]
#![deny(clippy::float_arithmetic)]
//! Núcleo del POS.
//!
//! Este crate no conoce SQLite, la interfaz gráfica ni la impresora. Contiene
//! únicamente las reglas de negocio que deben poder probarse de forma aislada:
//! dinero, cantidades, impuestos, ticket, descuentos, pagos, caja y stock.
//!
//! Regla dura: **nunca se usa `f64` para dinero**. Todos los importes viven en
//! [`Money`] (respaldado por `rust_decimal::Decimal`).

pub mod cash;
pub mod error;
pub mod ids;
pub mod model;
pub mod money;
pub mod payment;
pub mod permissions;
pub mod quantity;
pub mod rate;
pub mod receipt;
pub mod session;
pub mod stock;
pub mod ticket;

pub use cash::{CashCount, CashRounding, ChangeBreakdown, Denomination};
pub use error::CoreError;
pub use ids::{CategoryId, LineKey, LocationId, ProductId, SessionId, TaxCategoryId, TicketId, UserId};
pub use model::{Category, Location, PriceMode, Product, ProductSnapshot, Role, TaxCategory, User};
pub use money::Money;
pub use payment::{Payment, PaymentKind, PaymentMethod, PaymentMethodConfig, PaymentSummary, Payments};
pub use permissions::{Capability, RoleKind};
pub use quantity::{Qty, SaleUnit};
pub use rate::Rate;
pub use session::{PaymentTotal, SessionClose, SessionTotals};
pub use stock::{apply_delta, is_stock_line, sale_stock_movements, StockMovement, StockReason};
pub use ticket::{
    Checkout, Discount, DiscountAllocation, LineAmount, RemovedLine, SaleLine, SaleLineKind,
    TaxAmount, TicketDraft, TicketLine, TicketTotals,
};
