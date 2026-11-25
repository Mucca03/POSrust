use std::fmt;

macro_rules! string_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_id!(ProductId, "Identificador de producto (UUID heredado o nuevo).");
string_id!(CategoryId, "Identificador de categoría.");
string_id!(TaxCategoryId, "Identificador de categoría de impuesto.");
string_id!(UserId, "Identificador de usuario (`people.ID`).");
string_id!(TicketId, "Identificador del ticket/recibo (`tickets.ID` = `receipts.ID`).");
string_id!(SessionId, "Identificador de sesión de caja (`closedcash.MONEY`).");
string_id!(LocationId, "Identificador de bodega/local (`locations.ID`).");

/// Clave local de una línea dentro de un [`TicketDraft`](crate::ticket::TicketDraft).
///
/// No es el `LINE` de la base de datos: el store asigna ese consecutivo al
/// persistir. Sirve para que la interfaz pueda referirse a una línea sin
/// depender de su posición en el vector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineKey(u64);

impl LineKey {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for LineKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
