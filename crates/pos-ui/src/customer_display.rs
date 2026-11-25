use super::*;
use crate::classic;

impl App {
    pub(super) fn render_customer_display(&mut self, ctx: &Context) {
        if !self.settings.customer_display || self.customer_display_closed || self.user.is_none() {
            return;
        }
        let (label, amount) = match self.dialog.as_ref() {
            Some(Dialog::Change { total, amount, .. }) if self.sale_drawer.is_none() => (
                format!("Total pagado: {}", total.format_cop_exact()), Some(("Vueltas", *amount)),
            ),
            Some(Dialog::Change { total, .. }) => ("Pago confirmado".into(), Some(("Total", *total))),
            _ if !self.draft.is_empty() => {
                let detail = self.draft.lines().last().map(|line| line.product().name.clone()).unwrap_or_default();
                let amount = self.draft.totals().ok().map(|t| ("Total", t.total));
                (detail, amount)
            }
            _ => ("Bienvenido".to_owned(), None),
        };
        let title = if self.settings.brand_text.is_empty() {
            "POSrust"
        } else {
            self.settings.brand_text.lines().next().unwrap_or("POSrust")
        };
        self.customer_display_closed = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("pos-customer-display"),
            egui::ViewportBuilder::default().with_title("POSrust · Pantalla de cliente")
                .with_inner_size([420.0, 200.0]).with_min_inner_size([310.0, 170.0]).with_active(false),
            |ctx, class| {
                if class == egui::ViewportClass::Embedded { return false; }
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading(title);
                        ui.separator();
                        ui.label(RichText::new(&label).size(20.0));
                        if let Some((caption, amount)) = amount {
                            ui.strong(caption);
                            ui.label(RichText::new(amount.format_cop_exact()).size(39.0).strong().color(classic::BLUE));
                        }
                    });
                });
                ctx.input(|input| input.viewport().close_requested())
            },
        );
    }
}
