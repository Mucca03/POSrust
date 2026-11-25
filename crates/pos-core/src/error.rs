use crate::ids::LineKey;
use crate::money::Money;

/// Errores de dominio. No incluyen detalles de SQLite ni de la UI.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CoreError {
    #[error("importe inválido: {0}")]
    InvalidMoney(String),

    #[error("cantidad inválida: {0}")]
    InvalidQuantity(String),

    #[error("porcentaje inválido: {0}")]
    InvalidRate(String),

    #[error("el carrito está vacío")]
    EmptyCart,

    #[error("la línea {0} no existe")]
    LineNotFound(LineKey),

    #[error("la cantidad debe ser mayor que cero")]
    NonPositiveQuantity,

    #[error("los productos por unidad no admiten cantidades fraccionarias")]
    FractionalUnitQuantity,

    #[error("descuento inválido: {0}")]
    InvalidDiscount(String),

    #[error("el descuento supera la base sobre la que se aplica")]
    DiscountExceedsBase,

    #[error("el pago es insuficiente; faltan {0}")]
    InsufficientPayment(Money),

    #[error("solo el efectivo puede pagar más que el total")]
    NonCashOverpay,

    #[error("el efectivo recibido es menor que el importe aplicado")]
    CashTenderedTooLow,

    #[error("pago inválido: {0}")]
    InvalidPayment(String),

    #[error("solo se permiten redondeos de efectivo de 0, 50 o 100 COP")]
    InvalidRounding,

    #[error("el conteo de efectivo no admite cantidades negativas")]
    InvalidCashCount,

    #[error("el texto supera el máximo de {max} caracteres")]
    TextTooLong { max: usize },

    #[error("movimiento de stock inválido: {0}")]
    InvalidStockMovement(String),

    #[error("datos de sesión inválidos: {0}")]
    InvalidSession(String),
}
