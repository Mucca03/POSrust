use eframe::egui::{self, Align, Button, Color32, Key, Layout, RichText, Ui};
use pos_core::{
    CoreError, Denomination, Money, Payment, PaymentMethod, Payments, TicketDraft,
};

use crate::classic;

pub(crate) const METHODS: [PaymentMethod; 5] = [
    PaymentMethod::Cash,
    PaymentMethod::Nequi,
    PaymentMethod::Daviplata,
    PaymentMethod::Bancolombia,
    PaymentMethod::Magcard,
];

pub(crate) fn money_field(text: &str) -> Result<Money, CoreError> {
    // A decimal comma or point is accepted, but no thousands separator.
    // A mistyped extra digit must not silently change the payable amount.
    let text = text.trim();
    let mut parts = text.split(['.', ',']);
    let integer = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if integer.is_empty() || !integer.bytes().all(|x| x.is_ascii_digit())
        || fraction.is_some_and(|p| p.is_empty() || p.len() > 2 || !p.bytes().all(|x| x.is_ascii_digit()))
        || parts.next().is_some()
    {
        return Err(CoreError::InvalidMoney("usa hasta dos decimales, sin separador de miles".into()));
    }
    Money::parse(&text.replace(',', "."))
}

pub(crate) struct CheckoutForm {
    pub payments: Payments,
    method: PaymentMethod,
    amount: String,
    reference: String,
    error: Option<String>,
    focus_amount: bool,
}

impl Default for CheckoutForm {
    fn default() -> Self {
        Self {
            payments: Payments::new(),
            method: PaymentMethod::Cash,
            amount: String::new(),
            reference: String::new(),
            error: None,
            focus_amount: true,
        }
    }
}

pub(crate) enum CheckoutAction { Wait, Cancel, Save }

impl CheckoutForm {
    fn candidate_total(&self, draft: &TicketDraft) -> Result<Money, CoreError> {
        let unrounded = draft.totals_with_rounding(false)?.total;
        if self.payments.has_cash() || (self.method.is_cash() && self.payments.total_paid() < unrounded) {
            Ok(draft.totals_with_rounding(true)?.total)
        } else {
            Ok(unrounded)
        }
    }

    fn payment_to_add(&self, draft: &TicketDraft, exact: bool) -> Result<Payment, CoreError> {
        let due = self.candidate_total(draft)? - self.payments.total_paid();
        if !due.is_positive() {
            return Err(CoreError::InvalidPayment("no queda saldo para este medio".into()));
        }
        let entered = if exact { due } else { money_field(&self.amount)? };
        if self.method.is_cash() {
            Payment::cash(entered.min(due), entered)
        } else {
            if entered > due {
                return Err(CoreError::NonCashOverpay);
            }
            Payment::electronic(self.method.clone(), entered, Some(self.reference.clone()))
        }
    }

    fn add(&mut self, draft: &TicketDraft, exact: bool) -> Result<(), CoreError> {
        let payment = self.payment_to_add(draft, exact)?;
        self.payments.add(payment);
        self.amount.clear();
        self.reference.clear();
        Ok(())
    }

    fn preview(&self, draft: &TicketDraft) -> Option<pos_core::Checkout> {
        if let Ok(checkout) = draft.checkout(&self.payments) {
            return Some(checkout);
        }
        let mut payments = self.payments.clone();
        payments.add(self.payment_to_add(draft, false).ok()?);
        draft.checkout(&payments).ok()
    }

    pub fn show(&mut self, ui: &mut Ui, draft: &TicketDraft) -> CheckoutAction {
        classic::title(ui, "Cobrar ticket · COP");
        let split = ui.available_width();
        let side = 125.0;
        let pad = (split * 0.37).clamp(210.0, 276.0);
        let middle = split - side - pad - 2.0 * ui.spacing().item_spacing.x;
        let displayed = self.candidate_total(draft).unwrap_or(Money::ZERO);
        let due = (displayed - self.payments.total_paid()).max(Money::ZERO);
        let mut add = false;
        let mut undo = false;
        ui.horizontal(|ui| {
            let width = ((split - 115.0) / 2.0).max(185.0);
            amount_row(ui, "Total", displayed.format_cop_exact(), width, true);
            amount_row(ui, "Pendiente", due.format_cop_exact(), width, true);
            add = ui.add_enabled(due.is_positive(), Button::new(RichText::new("+").size(22.0))
                .min_size(egui::vec2(46.0, 35.0))).on_hover_text("Agregar pago").clicked();
            undo = ui.add_enabled(!self.payments.is_empty(), Button::new(RichText::new("−").size(22.0))
                .min_size(egui::vec2(46.0, 35.0))).on_hover_text("Quitar el último pago agregado").clicked();
        });
        if undo {
            self.payments.pop();
            self.reset_entry();
        }
        let mut enter = false;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(side, 1.0), Layout::top_down(Align::LEFT), |ui| {
                for method in METHODS {
                    let active = self.method == method;
                    let response = ui.add_sized([side, 36.0], Button::new(format!("     {}", method.label()))
                        .selected(active));
                    let mut icon = response.rect.center();
                    icon.x = response.rect.left() + 16.0;
                    classic::draw_icon(&ui.painter_at(response.rect), icon,
                        if method.is_cash() { classic::Icon::Drawer } else { classic::Icon::Payments },
                        classic::CYAN);
                    if response.clicked() && !active {
                        self.method = method;
                        self.reset_entry();
                    }
                }
            });
            let has_due = self.candidate_total(draft).is_ok_and(|total| total > self.payments.total_paid());
            ui.allocate_ui_with_layout(egui::vec2(middle, 1.0), Layout::top_down(Align::LEFT), |ui| {
                let change = self.preview(draft).map_or(Money::ZERO, |value| value.payments.change);
                let entered = money_field(&self.amount).map(|money| money.format_cop_exact()).unwrap_or_else(|_| "—".into());
                amount_row(ui, if self.method.is_cash() { "Recibido" } else { "Importe" }, entered, middle, false);
                amount_row(ui, "Devuelta", change.format_cop_exact(), middle, false);
                if self.method.is_cash() {
                    self.denominations(ui, middle, has_due);
                } else {
                    ui.label("Referencia (opcional)");
                    ui.add_enabled(has_due, egui::TextEdit::singleline(&mut self.reference)
                        .desired_width(middle - 8.0).char_limit(80));
                }
                ui.add_space(3.0);
                egui::ScrollArea::vertical().id_salt("pagos-agregados").max_height(56.0).show(ui, |ui| {
                    for payment in self.payments.entries() {
                        let received = payment.tendered.filter(|t| *t != payment.amount)
                            .map(|t| format!(" · recibido {}", t.format_cop_exact())).unwrap_or_default();
                        ui.label(format!("{}: {}{received}", payment.method.label(), payment.amount.format_cop_exact()));
                    }
                });
            });
            ui.allocate_ui_with_layout(egui::vec2(pad, 1.0), Layout::top_down(Align::LEFT), |ui| {
                ui.add_enabled_ui(has_due, |ui| self.keypad(ui, pad));
                let response = ui.add_enabled(has_due, egui::TextEdit::singleline(&mut self.amount)
                    .id_salt("payment-amount").hint_text("Digita el valor en COP")
                    .horizontal_align(Align::RIGHT).desired_width(pad - 8.0).char_limit(20));
                if std::mem::take(&mut self.focus_amount) && has_due {
                    response.request_focus();
                }
                if response.changed() {
                    self.error = None;
                    ui.ctx().request_repaint();
                }
                enter = has_due && response.lost_focus() && first_enter(ui);
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
            });
        });
        ui.separator();
        let mut focus_confirm = false;
        let complete = self.preview(draft).is_some();
        ui.horizontal(|ui| {
            let exact = ui.add_enabled(due.is_positive(), Button::new("Agregar exacto").min_size(egui::vec2(110.0, 36.0))).clicked();
            ui.weak("Tirilla manual · Último pago");
            if exact || add || enter {
                self.error = self.add(draft, exact).err().map(|error| error.to_string());
                focus_confirm = self.error.is_none() && draft.checkout(&self.payments).is_ok();
                self.focus_amount = !focus_confirm;
                ui.ctx().request_repaint();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let save = ui.add_enabled(complete || focus_confirm, Button::new(
                    RichText::new("Confirmar venta").strong().color(Color32::WHITE))
                    .fill(Color32::from_rgb(64, 129, 188)).min_size(egui::vec2(128.0, 38.0)));
                if focus_confirm { save.request_focus(); }
                let cancel = ui.add(Button::new("Cancelar").min_size(egui::vec2(96.0, 38.0)));
                if cancel.clicked() {
                    return CheckoutAction::Cancel;
                }
                if save.clicked() && (save.clicked_by(egui::PointerButton::Primary) || (!enter && first_enter(ui))) {
                    if draft.checkout(&self.payments).is_err() {
                        if let Err(error) = self.add(draft, false) {
                            self.fail(error.to_string());
                            return CheckoutAction::Wait;
                        }
                    }
                    return CheckoutAction::Save;
                }
                CheckoutAction::Wait
            }).inner
        }).inner
    }

    fn reset_entry(&mut self) {
        self.amount.clear();
        self.reference.clear();
        self.error = None;
        self.focus_amount = true;
    }

    fn denominations(&mut self, ui: &mut Ui, width: f32, enabled: bool) {
        let gap = ui.spacing().item_spacing.x;
        let width = (width - 3.0 * gap) / 4.0;
        for row in Denomination::ALL.chunks(4) {
            ui.horizontal(|ui| {
                for denomination in row {
                    let button = Button::new(RichText::new(denomination.label()).color(classic::BLUE).size(17.0))
                        .min_size(egui::vec2(width, 45.0));
                    if ui.add_enabled(enabled, button).clicked() {
                        let current = if self.amount.is_empty() { Ok(Money::ZERO) } else { money_field(&self.amount) };
                        match current {
                            Ok(current) => {
                                self.amount = (current + denomination.value()).to_plain_string();
                                self.error = None;
                            }
                            Err(error) => self.fail(error.to_string()),
                        }
                        self.focus_amount = true;
                        ui.ctx().request_repaint();
                    }
                }
            });
        }
    }

    fn keypad(&mut self, ui: &mut Ui, width: f32) {
        let gap = ui.spacing().item_spacing.x;
        let key_width = (width - gap * 2.0) / 3.0;
        let button = |text, wide| Button::new(RichText::new(text).size(22.0).color(classic::BLUE))
            .min_size(egui::vec2(if wide { key_width * 2.0 + gap } else { key_width }, 39.0));
        ui.horizontal(|ui| {
            if ui.add(button("CE", true)).on_hover_text("Limpiar importe").clicked() {
                self.amount.clear();
                self.error = None;
                self.focus_amount = true;
            }
            if ui.add(button("⌫", false)).on_hover_text("Borrar último dígito").clicked() {
                self.amount.pop();
                self.error = None;
                self.focus_amount = true;
            }
        });
        for row in [["1", "2", "3"], ["4", "5", "6"], ["7", "8", "9"]] {
            ui.horizontal(|ui| {
                for digit in row {
                    if ui.add(button(digit, false)).clicked() {
                        self.append_digit(digit);
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.add(button("0", true)).clicked() { self.append_digit("0"); }
            if ui.add(button(",", false)).clicked() && !self.amount.contains(['.', ',']) {
                if self.amount.is_empty() { self.amount.push('0'); }
                self.append_digit(",");
            }
        });
    }

    fn append_digit(&mut self, digit: &str) {
        if self.amount.len() < 20 {
            self.amount.push_str(digit);
            self.error = None;
            self.focus_amount = true;
        }
    }

    pub fn fail(&mut self, message: String) {
        self.error = Some(message);
    }
}

fn amount_row(ui: &mut Ui, label: &str, amount: String, width: f32, primary: bool) {
    let height = if primary { 37.0 } else { 30.0 };
    let label_width = width * 0.38;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    classic::cell(ui, rect.left() + 2.0, rect.top(), label_width - 7.0, height,
        RichText::new(label).size(if primary { 18.0 } else { 15.0 }), Align::LEFT);
    let value = egui::Rect::from_min_max(
        rect.left_top() + egui::vec2(label_width, 1.0), rect.right_bottom() - egui::vec2(2.0, 1.0),
    );
    ui.painter().rect_filled(value, 1.0, Color32::WHITE);
    ui.painter().rect_stroke(value, 1.0, egui::Stroke::new(1.0_f32, classic::BORDER));
    let mut text = RichText::new(amount).size(if primary { 21.0 } else { 16.0 });
    if primary { text = text.strong(); }
    classic::cell(ui, value.left() + 5.0, value.top(), value.width() - 10.0, value.height(), text, Align::RIGHT);
}

fn first_enter(ui: &Ui) -> bool {
    ui.input(|i| i.events.iter().any(|event| matches!(event,
        egui::Event::Key { key: Key::Enter, pressed: true, repeat: false, .. }
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pos_core::{ProductSnapshot, Qty, Rate, SaleUnit};

    fn draft() -> TicketDraft {
        let mut ticket = TicketDraft::new();
        ticket.add_snapshot(ProductSnapshot {
            product_id: "p".into(), name: "Producto".into(), code: "123".into(), unit: SaleUnit::Unit,
            tax_category_id: "100".into(), tax_rate: Rate::ZERO, unit_price_net: Money::from_pesos(7500),
        }, Qty::one()).unwrap();
        ticket
    }

    #[test]
    fn campo_monetario_no_redondea_ni_acepta_miles() {
        assert_eq!(money_field("1000,25").unwrap(), Money::from_cents(100025));
        for value in ["1.000", "1,000.00", "1,001", "10,", "-1", ""] {
            assert!(money_field(value).is_err(), "{value}");
        }
    }

    #[test]
    fn cobro_mixto_con_devuelta_y_sin_exceso_electronico() {
        let draft = draft();
        let mut form = CheckoutForm {
            method: PaymentMethod::Nequi, amount: "8000".into(), ..Default::default()
        };
        assert_eq!(form.add(&draft, false), Err(CoreError::NonCashOverpay));
        assert!(form.payments.is_empty());
        form.amount = "2500".into();
        form.add(&draft, false).unwrap();
        assert!(draft.checkout(&form.payments).is_err());
        form.method = PaymentMethod::Cash;
        form.amount = "10000".into();
        form.add(&draft, false).unwrap();
        assert_eq!(form.payments.total_paid(), Money::from_pesos(7500));
        assert_eq!(draft.checkout(&form.payments).unwrap().payments.change, Money::from_pesos(5000));
    }
}
