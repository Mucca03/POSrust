// Column order matches the supported MySQL export schema.
use crate::schema::ColumnKind;

pub struct Column { pub name: &'static str, pub kind: ColumnKind }
pub struct TableSpec { pub name: &'static str, pub columns: &'static [Column] }

pub const APPLICATIONS_COLUMNS: &[Column] = &[
    Column { name: "ID", kind: ColumnKind::Text },
    Column { name: "NAME", kind: ColumnKind::Text },
    Column { name: "VERSION", kind: ColumnKind::Text },
    Column { name: "INSTDATE", kind: ColumnKind::Timestamp },
];

pub const CATEGORIES_COLUMNS: &[Column] = &[
    Column { name: "ID", kind: ColumnKind::Text },
    Column { name: "NAME", kind: ColumnKind::Text },
    Column { name: "PARENTID", kind: ColumnKind::Text },
    Column { name: "IMAGE", kind: ColumnKind::Blob },
    Column { name: "TEXTTIP", kind: ColumnKind::Text },
    Column { name: "CATSHOWNAME", kind: ColumnKind::Int },
    Column { name: "CATORDER", kind: ColumnKind::Text },
];

pub const CLOSEDCASH_COLUMNS: &[Column] = &[
    Column { name: "MONEY", kind: ColumnKind::Text },
    Column { name: "HOST", kind: ColumnKind::Text },
    Column { name: "HOSTSEQUENCE", kind: ColumnKind::Int },
    Column { name: "DATESTART", kind: ColumnKind::DateTime },
    Column { name: "DATEEND", kind: ColumnKind::DateTime },
    Column { name: "NOSALES", kind: ColumnKind::Int },
];

pub const CUSTOMERS_COLUMNS: &[Column] = &[
    Column { name: "ID", kind: ColumnKind::Text },
    Column { name: "SEARCHKEY", kind: ColumnKind::Text },
    Column { name: "TAXID", kind: ColumnKind::Text },
    Column { name: "NAME", kind: ColumnKind::Text },
    Column { name: "TAXCATEGORY", kind: ColumnKind::Text },
    Column { name: "CARD", kind: ColumnKind::Text },
    Column { name: "MAXDEBT", kind: ColumnKind::Decimal },
    Column { name: "ADDRESS", kind: ColumnKind::Text },
    Column { name: "ADDRESS2", kind: ColumnKind::Text },
    Column { name: "POSTAL", kind: ColumnKind::Text },
    Column { name: "CITY", kind: ColumnKind::Text },
    Column { name: "REGION", kind: ColumnKind::Text },
    Column { name: "COUNTRY", kind: ColumnKind::Text },
    Column { name: "FIRSTNAME", kind: ColumnKind::Text },
    Column { name: "LASTNAME", kind: ColumnKind::Text },
    Column { name: "EMAIL", kind: ColumnKind::Text },
    Column { name: "PHONE", kind: ColumnKind::Text },
    Column { name: "PHONE2", kind: ColumnKind::Text },
    Column { name: "FAX", kind: ColumnKind::Text },
    Column { name: "NOTES", kind: ColumnKind::Text },
    Column { name: "VISIBLE", kind: ColumnKind::Bit },
    Column { name: "CURDATE", kind: ColumnKind::DateTime },
    Column { name: "CURDEBT", kind: ColumnKind::Decimal },
    Column { name: "IMAGE", kind: ColumnKind::Blob },
    Column { name: "ISVIP", kind: ColumnKind::Bit },
    Column { name: "DISCOUNT", kind: ColumnKind::Decimal },
    Column { name: "MEMODATE", kind: ColumnKind::DateTime },
];

pub const DRAWEROPENED_COLUMNS: &[Column] = &[
    Column { name: "OPENDATE", kind: ColumnKind::Timestamp },
    Column { name: "NAME", kind: ColumnKind::Text },
    Column { name: "TICKETID", kind: ColumnKind::Text },
];

pub const LINEREMOVED_COLUMNS: &[Column] = &[
    Column { name: "REMOVEDDATE", kind: ColumnKind::Timestamp },
    Column { name: "NAME", kind: ColumnKind::Text },
    Column { name: "TICKETID", kind: ColumnKind::Text },
    Column { name: "PRODUCTID", kind: ColumnKind::Text },
    Column { name: "PRODUCTNAME", kind: ColumnKind::Text },
    Column { name: "UNITS", kind: ColumnKind::Decimal },
];

pub const LOCATIONS_COLUMNS: &[Column] = &[
    Column { name: "ID", kind: ColumnKind::Text },
    Column { name: "NAME", kind: ColumnKind::Text },
    Column { name: "ADDRESS", kind: ColumnKind::Text },
];

pub const PAYMENTS_COLUMNS: &[Column] = &[
    Column { name: "ID", kind: ColumnKind::Text },
    Column { name: "RECEIPT", kind: ColumnKind::Text },
    Column { name: "PAYMENT", kind: ColumnKind::Text },
    Column { name: "TOTAL", kind: ColumnKind::Decimal },
    Column { name: "TIP", kind: ColumnKind::Decimal },
    Column { name: "TRANSID", kind: ColumnKind::Text },
    Column { name: "ISPROCESSED", kind: ColumnKind::Bit },
    Column { name: "RETURNMSG", kind: ColumnKind::Blob },
    Column { name: "NOTES", kind: ColumnKind::Text },
    Column { name: "TENDERED", kind: ColumnKind::Decimal },
    Column { name: "CARDNAME", kind: ColumnKind::Text },
    Column { name: "VOUCHER", kind: ColumnKind::Text },
];

pub const PEOPLE_COLUMNS: &[Column] = &[
    Column { name: "ID", kind: ColumnKind::Text },
    Column { name: "NAME", kind: ColumnKind::Text },
    Column { name: "APPPASSWORD", kind: ColumnKind::Text },
    Column { name: "CARD", kind: ColumnKind::Text },
    Column { name: "ROLE", kind: ColumnKind::Text },
    Column { name: "VISIBLE", kind: ColumnKind::Bit },
    Column { name: "IMAGE", kind: ColumnKind::Blob },
];

pub const PRODUCTS_COLUMNS: &[Column] = &[
    Column { name: "ID", kind: ColumnKind::Text },
    Column { name: "REFERENCE", kind: ColumnKind::Text },
    Column { name: "CODE", kind: ColumnKind::Text },
    Column { name: "CODETYPE", kind: ColumnKind::Text },
    Column { name: "NAME", kind: ColumnKind::Text },
    Column { name: "PRICEBUY", kind: ColumnKind::Decimal },
    Column { name: "PRICESELL", kind: ColumnKind::Decimal },
    Column { name: "CATEGORY", kind: ColumnKind::Text },
    Column { name: "TAXCAT", kind: ColumnKind::Text },
    Column { name: "ATTRIBUTESET_ID", kind: ColumnKind::Text },
    Column { name: "STOCKCOST", kind: ColumnKind::Decimal },
    Column { name: "STOCKVOLUME", kind: ColumnKind::Decimal },
    Column { name: "IMAGE", kind: ColumnKind::Blob },
    Column { name: "ISCOM", kind: ColumnKind::Bit },
    Column { name: "ISSCALE", kind: ColumnKind::Bit },
    Column { name: "ISCONSTANT", kind: ColumnKind::Bit },
    Column { name: "PRINTKB", kind: ColumnKind::Bit },
    Column { name: "SENDSTATUS", kind: ColumnKind::Bit },
    Column { name: "ISSERVICE", kind: ColumnKind::Bit },
    Column { name: "ATTRIBUTES", kind: ColumnKind::Blob },
    Column { name: "DISPLAY", kind: ColumnKind::Text },
    Column { name: "ISVPRICE", kind: ColumnKind::Int },
    Column { name: "ISVERPATRIB", kind: ColumnKind::Int },
    Column { name: "TEXTTIP", kind: ColumnKind::Text },
    Column { name: "WARRANTY", kind: ColumnKind::Int },
    Column { name: "STOCKUNITS", kind: ColumnKind::Decimal },
    Column { name: "PRINTTO", kind: ColumnKind::Text },
    Column { name: "warning", kind: ColumnKind::Timestamp },
    Column { name: "expiry", kind: ColumnKind::Timestamp },
    Column { name: "SUPPLIER", kind: ColumnKind::Text },
    Column { name: "UOM", kind: ColumnKind::Text },
    Column { name: "MEMODATE", kind: ColumnKind::DateTime },
    Column { name: "PRICESELL1", kind: ColumnKind::Decimal },
    Column { name: "PRICESELL2", kind: ColumnKind::Decimal },
    Column { name: "PRICESELL3", kind: ColumnKind::Decimal },
    Column { name: "QTY1", kind: ColumnKind::Decimal },
    Column { name: "QTY2", kind: ColumnKind::Decimal },
    Column { name: "QTY3", kind: ColumnKind::Decimal },
];

pub const PRODUCTS_CAT_COLUMNS: &[Column] = &[
    Column { name: "PRODUCT", kind: ColumnKind::Text },
    Column { name: "CATORDER", kind: ColumnKind::Int },
];

pub const RECEIPTS_COLUMNS: &[Column] = &[
    Column { name: "ID", kind: ColumnKind::Text },
    Column { name: "MONEY", kind: ColumnKind::Text },
    Column { name: "DATENEW", kind: ColumnKind::DateTime },
    Column { name: "ATTRIBUTES", kind: ColumnKind::Blob },
    Column { name: "PERSON", kind: ColumnKind::Text },
];

pub const ROLES_COLUMNS: &[Column] = &[
    Column { name: "ID", kind: ColumnKind::Text },
    Column { name: "NAME", kind: ColumnKind::Text },
    Column { name: "PERMISSIONS", kind: ColumnKind::Blob },
];

pub const STOCKCURRENT_COLUMNS: &[Column] = &[
    Column { name: "LOCATION", kind: ColumnKind::Text },
    Column { name: "PRODUCT", kind: ColumnKind::Text },
    Column { name: "ATTRIBUTESETINSTANCE_ID", kind: ColumnKind::Text },
    Column { name: "UNITS", kind: ColumnKind::Decimal },
];

pub const STOCKDIARY_COLUMNS: &[Column] = &[
    Column { name: "ID", kind: ColumnKind::Text },
    Column { name: "DATENEW", kind: ColumnKind::DateTime },
    Column { name: "REASON", kind: ColumnKind::Int },
    Column { name: "LOCATION", kind: ColumnKind::Text },
    Column { name: "PRODUCT", kind: ColumnKind::Text },
    Column { name: "ATTRIBUTESETINSTANCE_ID", kind: ColumnKind::Text },
    Column { name: "UNITS", kind: ColumnKind::Decimal },
    Column { name: "PRICE", kind: ColumnKind::Decimal },
    Column { name: "APPUSER", kind: ColumnKind::Text },
    Column { name: "SUPPLIER", kind: ColumnKind::Text },
    Column { name: "SUPPLIERDOC", kind: ColumnKind::Text },
];

pub const TAXCATEGORIES_COLUMNS: &[Column] = &[
    Column { name: "ID", kind: ColumnKind::Text },
    Column { name: "NAME", kind: ColumnKind::Text },
];

pub const TAXCUSTCATEGORIES_COLUMNS: &[Column] = &[
    Column { name: "ID", kind: ColumnKind::Text },
    Column { name: "NAME", kind: ColumnKind::Text },
];

pub const TAXES_COLUMNS: &[Column] = &[
    Column { name: "ID", kind: ColumnKind::Text },
    Column { name: "NAME", kind: ColumnKind::Text },
    Column { name: "CATEGORY", kind: ColumnKind::Text },
    Column { name: "CUSTCATEGORY", kind: ColumnKind::Text },
    Column { name: "PARENTID", kind: ColumnKind::Text },
    Column { name: "RATE", kind: ColumnKind::Decimal },
    Column { name: "RATECASCADE", kind: ColumnKind::Bit },
    Column { name: "RATEORDER", kind: ColumnKind::Int },
];

pub const TAXLINES_COLUMNS: &[Column] = &[
    Column { name: "ID", kind: ColumnKind::Text },
    Column { name: "RECEIPT", kind: ColumnKind::Text },
    Column { name: "TAXID", kind: ColumnKind::Text },
    Column { name: "BASE", kind: ColumnKind::Decimal },
    Column { name: "AMOUNT", kind: ColumnKind::Decimal },
];

pub const TICKETLINES_COLUMNS: &[Column] = &[
    Column { name: "TICKET", kind: ColumnKind::Text },
    Column { name: "LINE", kind: ColumnKind::Int },
    Column { name: "PRODUCT", kind: ColumnKind::Text },
    Column { name: "ATTRIBUTESETINSTANCE_ID", kind: ColumnKind::Text },
    Column { name: "UNITS", kind: ColumnKind::Decimal },
    Column { name: "PRICE", kind: ColumnKind::Decimal },
    Column { name: "TAXID", kind: ColumnKind::Text },
    Column { name: "ATTRIBUTES", kind: ColumnKind::Blob },
];

pub const TICKETS_COLUMNS: &[Column] = &[
    Column { name: "ID", kind: ColumnKind::Text },
    Column { name: "TICKETTYPE", kind: ColumnKind::Int },
    Column { name: "TICKETID", kind: ColumnKind::Int },
    Column { name: "PERSON", kind: ColumnKind::Text },
    Column { name: "CUSTOMER", kind: ColumnKind::Text },
    Column { name: "STATUS", kind: ColumnKind::Int },
];

pub const TICKETSNUM_COLUMNS: &[Column] = &[
    Column { name: "ID", kind: ColumnKind::Int },
];

pub const TICKETSNUM_PAYMENT_COLUMNS: &[Column] = &[
    Column { name: "ID", kind: ColumnKind::Int },
];

pub const TICKETSNUM_REFUND_COLUMNS: &[Column] = &[
    Column { name: "ID", kind: ColumnKind::Int },
];

pub const TABLES: &[TableSpec] = &[
    TableSpec { name: "applications", columns: APPLICATIONS_COLUMNS },
    TableSpec { name: "categories", columns: CATEGORIES_COLUMNS },
    TableSpec { name: "closedcash", columns: CLOSEDCASH_COLUMNS },
    TableSpec { name: "customers", columns: CUSTOMERS_COLUMNS },
    TableSpec { name: "draweropened", columns: DRAWEROPENED_COLUMNS },
    TableSpec { name: "lineremoved", columns: LINEREMOVED_COLUMNS },
    TableSpec { name: "locations", columns: LOCATIONS_COLUMNS },
    TableSpec { name: "payments", columns: PAYMENTS_COLUMNS },
    TableSpec { name: "people", columns: PEOPLE_COLUMNS },
    TableSpec { name: "products", columns: PRODUCTS_COLUMNS },
    TableSpec { name: "products_cat", columns: PRODUCTS_CAT_COLUMNS },
    TableSpec { name: "receipts", columns: RECEIPTS_COLUMNS },
    TableSpec { name: "roles", columns: ROLES_COLUMNS },
    TableSpec { name: "stockcurrent", columns: STOCKCURRENT_COLUMNS },
    TableSpec { name: "stockdiary", columns: STOCKDIARY_COLUMNS },
    TableSpec { name: "taxcategories", columns: TAXCATEGORIES_COLUMNS },
    TableSpec { name: "taxcustcategories", columns: TAXCUSTCATEGORIES_COLUMNS },
    TableSpec { name: "taxes", columns: TAXES_COLUMNS },
    TableSpec { name: "taxlines", columns: TAXLINES_COLUMNS },
    TableSpec { name: "ticketlines", columns: TICKETLINES_COLUMNS },
    TableSpec { name: "tickets", columns: TICKETS_COLUMNS },
    TableSpec { name: "ticketsnum", columns: TICKETSNUM_COLUMNS },
    TableSpec { name: "ticketsnum_payment", columns: TICKETSNUM_PAYMENT_COLUMNS },
    TableSpec { name: "ticketsnum_refund", columns: TICKETSNUM_REFUND_COLUMNS },
];
