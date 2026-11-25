use pos_core::{
    Discount, Money, Payment, PaymentMethod, Payments, PriceMode, Product, Qty, Rate, SaleUnit,
    TicketDraft,
};
use pos_core::ids::{ProductId, TaxCategoryId};
use proptest::prelude::*;

fn category_for(tax_percent: &str) -> &'static str {
    match tax_percent {
        "0" => "100",
        "5" => "101",
        "19" => "102",
        _ => "103",
    }
}

fn product(id: &str, price: i64, tax_percent: &str) -> Product {
    Product {
        id: ProductId::new(id),
        code: format!("C-{id}"),
        reference: format!("R-{id}"),
        name: format!("Producto {id}"),
        category_id: None,
        tax_category_id: TaxCategoryId::new(category_for(tax_percent)),
        tax_rate: Rate::parse_percent(tax_percent).unwrap(),
        list_price: Money::from_pesos(price),
        cost: Money::from_pesos(0),
        unit: SaleUnit::Unit,
        active: true,
        stock: None,
    }
}

proptest! {
    #[test]
    fn los_totales_siempre_cuadran(
        price1 in 1i64..1_000_000,
        price2 in 1i64..1_000_000,
        qty1 in 1i64..10,
        qty2 in 1i64..10,
        tax1 in prop::sample::select(vec!["0", "5", "19", "8"]),
        tax2 in prop::sample::select(vec!["0", "5", "19", "8"]),
        discount in 0u32..=100,
    ) {
        let p1 = product("p1", price1, tax1);
        let p2 = product("p2", price2, tax2);

        let mut draft = TicketDraft::new();
        draft
            .add_product(&p1, Qty::from_units(qty1), PriceMode::TaxExcluded)
            .unwrap();
        draft
            .add_product(&p2, Qty::from_units(qty2), PriceMode::TaxExcluded)
            .unwrap();

        if discount > 0 {
            draft
                .set_ticket_discount(Some(Discount::percent(
                    Rate::from_percent(rust_decimal::Decimal::from(discount)).unwrap(),
                )))
                .unwrap();
        }

        let totals = draft.totals().unwrap();

        prop_assert!(!totals.net.is_negative());
        prop_assert!(!totals.tax.is_negative());
        prop_assert!(!totals.total.is_negative());
        prop_assert_eq!(totals.total, totals.net + totals.tax);
        prop_assert_eq!(
            totals.tax_breakdown.iter().map(|t| t.base).sum::<Money>(),
            totals.net
        );
        prop_assert_eq!(
            totals.tax_breakdown.iter().map(|t| t.tax).sum::<Money>(),
            totals.tax
        );
        prop_assert_eq!(
            totals.line_amounts.iter().map(|l| l.net).sum::<Money>()
                - totals.ticket_discount,
            totals.net
        );
        prop_assert!(
            totals.ticket_discount_allocations.iter().map(|a| a.amount).sum::<Money>()
                <= totals.line_amounts.iter().map(|l| l.net).sum::<Money>()
        );
        prop_assert_eq!(
            totals.ticket_discount_allocations.iter().map(|a| a.amount).sum::<Money>(),
            totals.ticket_discount
        );

        let lines = draft.sale_lines_for(&totals);
        prop_assert_eq!(
            lines.iter().map(|l| l.net_total()).sum::<Money>(),
            totals.net
        );
    }

    #[test]
    fn pago_mixto_cuadra(
        total_pesos in 1i64..2_000_000,
        non_cash_pesos in 0i64..2_000_000,
        cash_tendered_pesos in 0i64..2_000_000,
    ) {
        let total = Money::from_pesos(total_pesos);
        let non_cash = Money::from_pesos(non_cash_pesos.min(total_pesos));
        let cash_due = total - non_cash;
        let cash_due_pesos = cash_due.to_cents() / 100;
        let tendered = if cash_due.is_zero() {
            Money::ZERO
        } else {
            Money::from_pesos(cash_tendered_pesos.max(cash_due_pesos))
        };

        let mut payments = Payments::new();
        if !non_cash.is_zero() {
            payments.add(
                Payment::electronic(PaymentMethod::Nequi, non_cash, None).unwrap(),
            );
        }
        if !cash_due.is_zero() {
            payments.add(Payment::cash(cash_due, tendered).unwrap());
        }

        let summary = payments.summary(total).unwrap();
        prop_assert_eq!(summary.total, total);
        prop_assert_eq!(summary.remaining, Money::ZERO);
        prop_assert_eq!(
            summary.change,
            if cash_due.is_zero() {
                Money::ZERO
            } else {
                tendered - cash_due
            }
        );
        prop_assert!(summary.is_complete());
    }
}
