use eframe::egui::{Align, Button, ComboBox, Grid, Layout, ScrollArea, TextEdit, Ui};
use pos_core::{model::TAX_CATEGORY_SIN_IVA, CategoryId, Product, TaxCategoryId};
use pos_store::{catalog::normalize_code, create_product, update_product, NewProduct};

use super::*;
use crate::classic;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Comparison {
    #[default]
    Any,
    Equal,
    AtLeast,
    AtMost,
}

impl Comparison {
    fn label(self) -> &'static str {
        match self {
            Self::Any => "Cualquiera",
            Self::Equal => "Igual a",
            Self::AtLeast => "Desde",
            Self::AtMost => "Hasta",
        }
    }

    fn test(self, field: &str) -> Result<impl Fn(Money) -> bool, pos_core::CoreError> {
        let amount = if self == Self::Any { Money::ZERO } else { money_field(field)? };
        Ok(move |price| match self {
            Self::Any => true, Self::Equal => price == amount,
            Self::AtLeast => price >= amount, Self::AtMost => price <= amount,
        })
    }
}

#[derive(Default)]
struct ProductFilter {
    barcode: String,
    query: String,
    category: Option<CategoryId>,
    cost_op: Comparison,
    cost: String,
    price_op: Comparison,
    price: String,
}

impl ProductFilter {
    fn show_stock(&mut self, ui: &mut Ui, catalog: &Catalog, focus: &mut bool) -> bool {
        let mut enter = false;
        let available = ui.available_width();
        let width = (available * 0.26).clamp(108.0, 325.0);
        let price_width = (available * 0.14).clamp(80.0, 180.0);
        Grid::new("stock-product-filters").spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label("Categoría");
            categories(ui, "stock-filter-category", &mut self.category, catalog, "Todas", width);
            ui.label("Costo (COP)");
            enter |= price_filter(ui, "stock-filter-cost", &mut self.cost_op, &mut self.cost, price_width);
            ui.end_row();
            ui.label("Código de barras");
            let code = ui.add(TextEdit::singleline(&mut self.barcode).desired_width(width).char_limit(128));
            enter |= code.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            ui.label("Precio sin IVA");
            enter |= price_filter(ui, "stock-filter-price", &mut self.price_op, &mut self.price, price_width);
            ui.end_row();
            ui.label("Nombre o referencia");
            let name = ui.add(TextEdit::singleline(&mut self.query).desired_width(width).char_limit(128));
            if std::mem::take(focus) { name.request_focus(); }
            enter |= name.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if ui.button("Limpiar filtros").clicked() {
                *self = Self::default();
                *focus = true;
                enter = true;
            }
            ui.end_row();
        });
        enter
    }

    fn show(&mut self, ui: &mut Ui, catalog: &Catalog, focus: &mut bool) -> bool {
        let width = (ui.available_width() - 115.0).clamp(136.0, 260.0);
        let mut enter = false;
        Grid::new("product-filters").spacing([10.0, 4.0]).show(ui, |ui| {
            ui.label("Código de barras");
            let barcode = ui.add(TextEdit::singleline(&mut self.barcode).desired_width(width).char_limit(128));
            enter |= barcode.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            ui.end_row();
            ui.label("Nombre o referencia");
            let name = ui.add(TextEdit::singleline(&mut self.query).desired_width(width).char_limit(128));
            if std::mem::take(focus) { name.request_focus(); }
            enter |= name.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            ui.end_row();
            ui.label("Categoría");
            categories(ui, "filtrar-categoria", &mut self.category, catalog, "Todas", width);
            ui.end_row();
            for (label, id, op, amount) in [
                ("Costo (COP)", "filtrar-costo", &mut self.cost_op, &mut self.cost),
                ("Precio sin IVA", "filtrar-precio", &mut self.price_op, &mut self.price),
            ] {
                ui.label(label);
                enter |= price_filter(ui, id, op, amount, (width - 116.0).max(42.0));
                ui.end_row();
            }
        });
        ui.horizontal(|ui| {
            let search = ui.add(Button::new("Buscar productos").min_size(egui::vec2(145.0, 33.0))).clicked();
            if ui.button("Limpiar filtros").clicked() {
                *self = Self::default();
                *focus = true;
                true
            } else {
                search || enter
            }
        }).inner
    }

    fn results(&self, catalog: &Catalog) -> Result<Vec<ProductId>, pos_core::CoreError> {
        let cost = self.cost_op.test(&self.cost)?;
        let price = self.price_op.test(&self.price)?;
        let code = normalize_code(&self.barcode);
        let matches = |p: &Product| {
            (code.is_empty() || normalize_code(&p.code).contains(&code))
                && self.category.as_ref().map_or(true, |category| p.category_id.as_ref() == Some(category))
                && cost(p.cost) && price(p.unit_price_net(catalog.price_mode))
        };
        let products = if self.query.trim().is_empty() {
            catalog.products.iter().collect()
        } else {
            catalog.search(&self.query, catalog.products.len())
        };
        Ok(products.into_iter().filter(|p| matches(p)).map(|p| p.id.clone()).collect())
    }
}

fn price_filter(ui: &mut Ui, id: &'static str, op: &mut Comparison, amount: &mut String, width: f32) -> bool {
    ui.horizontal(|ui| {
        ComboBox::from_id_salt(id).width(106.0).selected_text(op.label()).show_ui(ui, |ui| {
            for value in [Comparison::Any, Comparison::Equal, Comparison::AtLeast, Comparison::AtMost] {
                ui.selectable_value(op, value, value.label());
            }
        });
        let value = ui.add_enabled(*op != Comparison::Any,
            TextEdit::singleline(amount).desired_width(width).char_limit(20));
        value.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
    }).inner
}

#[derive(Default)]
pub(super) struct ProductSearch {
    filters: ProductFilter,
    results: Vec<ProductId>,
    selected: Option<ProductId>,
    generation: u64,
}

pub(super) enum SearchAction { Wait, Cancel, Add(ProductId) }

impl ProductSearch {
    pub(super) fn new(prefill: &str, catalog: &Catalog) -> Self {
        let mut form = Self::default();
        if prefill.trim().chars().all(|c| c.is_ascii_digit()) {
            form.filters.barcode = prefill.to_owned();
        } else {
            form.filters.query = prefill.to_owned();
        }
        form.results = form.filters.results(catalog).unwrap_or_default();
        form.selected = form.results.first().cloned();
        form
    }

    fn refresh(&mut self, catalog: &Catalog, error: &mut Option<String>) {
        match self.filters.results(catalog) {
            Ok(results) => {
                self.selected = results.first().cloned();
                self.results = results;
                self.generation += 1;
                *error = None;
            }
            Err(e) => *error = Some(e.to_string()),
        }
    }

    pub(super) fn show(
        &mut self, ui: &mut Ui, catalog: &Catalog, focus: &mut bool,
        error: &mut Option<String>,
    ) -> SearchAction {
        classic::title(ui, "Buscar productos");
        let width = ui.available_width();
        let right = (width * 0.38).min(274.0);
        let height = (ui.ctx().screen_rect().height() - 347.0).clamp(124.0, 288.0);
        let mut add = None;
        ui.horizontal_top(|ui| {
            let left = width - right - ui.spacing().item_spacing.x;
            ui.allocate_ui_with_layout(egui::vec2(left, 1.0), Layout::top_down(Align::Min), |ui| {
                if self.filters.show(ui, catalog, focus) {
                    self.refresh(catalog, error);
                }
                if list(ui, catalog, &self.results, &mut self.selected, height, self.generation, None) {
                    add = self.selected.clone();
                }
            });
            ui.allocate_ui_with_layout(egui::vec2(right, 1.0), Layout::top_down(Align::Min), |ui| {
                if numeric_pad(ui, right, &mut self.filters.barcode, false, 128) {
                    self.refresh(catalog, error);
                }
                if let Some(p) = self.selected.as_ref().and_then(|id| catalog.product_by_id(id.as_str())) {
                    ui.add_space(8.0);
                    ui.strong(p.name.trim());
                    ui.label(p.list_price.format_cop_exact());
                    ui.weak(if p.unit.is_weighted() { "Por kg" } else { "Por unidad" });
                }
            });
        });
        if let Some(message) = error { ui.colored_label(ui.visuals().error_fg_color, message); }
        if let Some(id) = add { return SearchAction::Add(id); }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.add_enabled(self.selected.is_some(), primary("Agregar a la venta")).clicked() {
                if let Some(id) = &self.selected { return SearchAction::Add(id.clone()); }
            }
            if ui.button("Cancelar").clicked() { SearchAction::Cancel } else { SearchAction::Wait }
        }).inner
    }
}

pub(super) struct ProductsForm {
    filter: ProductFilter,
    results: Vec<ProductId>,
    selected: Option<ProductId>,
    editing: Option<(Option<ProductId>, ProductFields)>,
    focus_record: bool,
    show_stock: bool,
    message: Option<String>,
    generation: u64,
}

struct ProductFields {
    reference: String,
    barcode: String,
    name: String,
    category: Option<CategoryId>,
    tax: Option<TaxCategoryId>,
    price: String,
    cost: String,
    unit: SaleUnit,
    show: bool,
}

impl ProductFields {
    fn new(catalog: &Catalog, category: Option<CategoryId>) -> Self {
        Self {
            reference: String::new(), barcode: String::new(), name: String::new(), category,
            tax: catalog.tax(TAX_CATEGORY_SIN_IVA).or_else(|| catalog.taxes.first()).map(|tax| tax.id.clone()),
            price: String::new(), cost: "0".into(), unit: SaleUnit::Unit, show: true,
        }
    }

    fn from_product(product: &Product, catalog: &Catalog) -> Self {
        let gross = if catalog.price_mode == PriceMode::TaxIncluded { product.list_price }
            else { Money::from_decimal(product.list_price.as_decimal() * product.tax_rate.multiplier()) };
        Self {
            reference: product.reference.clone(), barcode: product.code.clone(), name: product.name.clone(),
            category: product.category_id.clone(), tax: Some(product.tax_category_id.clone()),
            price: gross.to_plain_string(), cost: product.cost.to_plain_string(), unit: product.unit,
            show: catalog.in_grid(&product.id),
        }
    }

    fn values(&self, price_mode: PriceMode, catalog: &Catalog) -> Result<NewProduct, Box<dyn Error>> {
        let category = self.category.clone().ok_or("selecciona una categoría")?;
        let tax = self.tax.clone().ok_or("selecciona un impuesto")?;
        let amount = money_field(&self.price)?;
        let price = if price_mode == PriceMode::TaxExcluded {
            let rate = catalog.tax(tax.as_str()).ok_or("impuesto no disponible")?.rate;
            Money::from_decimal(amount.as_decimal() / rate.multiplier())
        } else { amount };
        Ok(NewProduct {
            reference: self.reference.clone(), code: self.barcode.clone(), name: self.name.clone(),
            category, tax, price, cost: money_field(&self.cost)?, unit: self.unit, in_catalog: self.show,
        })
    }

    fn show(&mut self, ui: &mut Ui, catalog: &Catalog, focus: &mut bool) {
        let width = (ui.available_width() - 125.0).clamp(120.0, 355.0);
        let reference = |ui: &mut Ui, reference: &mut String, focus: &mut bool, size| {
            ui.add_sized([112.0, 27.0], egui::Label::new("Referencia *"));
            let response = ui.add(TextEdit::singleline(reference).desired_width(size).char_limit(128));
            if std::mem::take(focus) { response.request_focus(); }
        };
        let barcode = |ui: &mut Ui, barcode: &mut String, size| {
            ui.label("Código de barras *");
            ui.add(TextEdit::singleline(barcode).desired_width(size).char_limit(128));
        };
        if ui.available_width() >= 520.0 {
            ui.horizontal(|ui| {
                reference(ui, &mut self.reference, focus, 130.0);
                barcode(ui, &mut self.barcode, 130.0);
            });
        } else {
            ui.horizontal(|ui| reference(ui, &mut self.reference, focus, width));
            ui.horizontal(|ui| barcode(ui, &mut self.barcode, width));
        }
        Grid::new("new-product-fields").spacing([10.0, 7.0]).show(ui, |ui| {
            ui.add_sized([107.0, 27.0], egui::Label::new("Nombre *"));
            ui.add(TextEdit::singleline(&mut self.name).desired_width(width).char_limit(128));
            ui.end_row();
            ui.label("Categoría *");
            categories(ui, "nueva-categoria", &mut self.category, catalog, "Seleccionar", width);
            ui.end_row();
            ui.label("Impuesto *");
            let tax_name = self.tax.as_ref().and_then(|id| catalog.tax(id.as_str())).map_or("Seleccionar", |t| &t.name);
            ComboBox::from_id_salt("nuevo-impuesto").width(width).selected_text(tax_name).show_ui(ui, |ui| {
                for tax in &catalog.taxes {
                    ui.selectable_value(&mut self.tax, Some(tax.id.clone()), &tax.name);
                }
            });
            ui.end_row();
            ui.label("Precio con IVA *");
            ui.add(TextEdit::singleline(&mut self.price).hint_text("COP").desired_width(width).char_limit(20));
            ui.end_row();
            ui.label("Precio sin IVA");
            let net = money_field(&self.price).ok().and_then(|price| {
                let rate = catalog.tax(self.tax.as_ref()?.as_str())?.rate;
                Some(Money::from_decimal(price.as_decimal() / rate.multiplier()).format_cop_exact())
            });
            ui.label(net.as_deref().unwrap_or("—"));
            ui.end_row();
            ui.label("Costo (COP) *");
            ui.add(TextEdit::singleline(&mut self.cost).desired_width(width).char_limit(20));
            ui.end_row();
            ui.label("Unidad de venta");
            ComboBox::from_id_salt("product-unit").width(170.0)
                .selected_text(if self.unit.is_weighted() { "Kilogramo" } else { "Unidad" }).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.unit, SaleUnit::Unit, "Unidad");
                    ui.selectable_value(&mut self.unit, SaleUnit::Kilogram, "Kilogramo");
            });
            ui.end_row();
            ui.label("Catálogo");
            ui.checkbox(&mut self.show, "Mostrar en ventas");
            ui.end_row();
        });
    }
}

impl ProductsForm {
    pub(super) fn new(catalog: &Catalog) -> Self {
        let filter = ProductFilter::default();
        let results = filter.results(catalog).unwrap_or_default();
        let selected = results.first().cloned();
        Self { filter, results, selected, editing: None, focus_record: false, show_stock: false,
            message: None, generation: 0 }
    }

    pub(super) fn is_editing(&self) -> bool { self.editing.is_some() }

    pub(super) fn show(&mut self, app: &mut App, ui: &mut Ui) -> Result<(), Box<dyn Error>> {
        let editing = self.editing.is_some();
        let mut search = ui.add_enabled_ui(!editing, |ui| {
            self.filter.show_stock(ui, &app.catalog, &mut app.focus_dialog)
        }).inner;
        let width = (ui.available_width() * 0.30).clamp(168.0, 288.0);
        let position = self.selected.as_ref().and_then(|id| self.results.iter().position(|p| p == id));
        let mut go = None;
        let mut save = false;
        ui.add_space(5.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("{} productos", self.results.len()));
            ui.weak(format!("{} / {}", position.map_or(0, |i| i + 1), self.results.len()));
            let pos = position.unwrap_or(0);
            let last = self.results.len().saturating_sub(1);
            for (label, hint, index, enabled) in [
                ("«", "Primer producto", 0, pos > 0), ("‹", "Producto anterior", pos.saturating_sub(1), pos > 0),
                ("›", "Producto siguiente", pos + 1, pos < last), ("»", "Último producto", last, pos < last),
            ] {
                if ui.add_enabled(!editing && enabled, Button::new(label).min_size(egui::vec2(31.0, 33.0)))
                    .on_hover_text(hint).clicked() { go = Some(index); }
            }
            search |= ui.add_enabled(!editing, Button::new("Buscar productos").min_size(egui::vec2(105.0, 33.0))).clicked();
            if ui.add_enabled(!editing, Button::new("+ Nuevo producto").min_size(egui::vec2(112.0, 33.0))).clicked() {
                self.editing = Some((None, ProductFields::new(&app.catalog, self.filter.category.clone())));
                self.focus_record = true;
                self.show_stock = false;
                self.message = None;
                app.dialog_error = None;
            }
            if ui.add_enabled(!editing && self.selected.is_some(), Button::new("Editar").min_size(egui::vec2(57.0, 33.0))).clicked() {
                if let Some(product) = self.selected.as_ref().and_then(|id| app.catalog.product_by_id(id.as_str())) {
                    self.editing = Some((Some(product.id.clone()), ProductFields::from_product(product, &app.catalog)));
                    self.focus_record = true;
                    self.show_stock = false;
                    self.message = None;
                    app.dialog_error = None;
                }
            }
            save = ui.add_enabled(editing, primary("Guardar producto")).clicked();
            if editing && ui.button("Cancelar").clicked() {
                self.editing = None;
                self.focus_record = false;
                app.dialog_error = None;
            }
        });
        if search {
            self.results = self.filter.results(&app.catalog)?;
            self.selected = self.results.first().cloned();
            self.generation += 1;
            app.dialog_error = None;
        }
        if let Some(index) = go {
            self.selected = self.results.get(index).cloned();
        }
        if let Some(message) = &self.message {
            ui.strong(message);
        }
        ui.separator();
        let height = (ui.available_height() - 26.0).max(96.0);
        let details_width = ui.available_width() - width - ui.spacing().item_spacing.x;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(width, height), Layout::top_down(Align::Min), |ui| {
                ui.set_width(width);
                ui.add_enabled_ui(self.editing.is_none(), |ui| {
                    list(ui, &app.catalog, &self.results, &mut self.selected, height - 14.0, self.generation, go);
                });
            });
            ui.allocate_ui_with_layout(egui::vec2(details_width, height), Layout::top_down(Align::Min), |ui| {
                ui.set_width(details_width);
                let selected = self.selected.as_ref().and_then(|id| app.catalog.product_by_id(id.as_str()));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.show_stock, false, "General");
                    if ui.add_enabled(self.editing.is_none() && selected.is_some(),
                        Button::new("Stock").selected(self.show_stock)).clicked() { self.show_stock = true; }
                    if let Some((id, _)) = &self.editing {
                        ui.weak(if id.is_some() { "Editando registro" } else { "Nuevo registro" });
                    }
                });
                ui.separator();
                ScrollArea::vertical().id_salt("product-details").max_height((height - 41.0).max(50.0)).show(ui, |ui| {
                    if let Some((_, form)) = &mut self.editing {
                        form.show(ui, &app.catalog, &mut self.focus_record);
                    } else if let Some(product) = selected {
                        if self.show_stock {
                            ui.label(format!("Ubicación: {}", app.catalog.location.name));
                            ui.strong(format!("Existencia: {}", product.stock.unwrap_or(Qty::ZERO).format(product.unit)));
                            ui.weak("Para ingresar mercancía, usa Stock Management.");
                        } else {
                            product_details(ui, product, &app.catalog);
                        }
                    } else {
                        ui.weak("No hay un producto seleccionado.");
                    }
                });
            });
        });
        if save {
            let Some((editing_id, form)) = &self.editing else { return Ok(()) };
            let product = form.values(app.catalog.price_mode, &app.catalog)?;
            let now = app.timestamp()?;
            let user = app.user.as_ref().ok_or("sesión no autenticada")?;
            let id = if let Some(id) = editing_id {
                if app.catalog.product_by_id(id.as_str()).is_some_and(|p| p.unit != product.unit)
                    && app.product_in_pending_sale(id) {
                    return Err("no se puede cambiar la unidad de un producto en una venta abierta".into());
                }
                update_product(&mut app.conn, id, &product, &user.id, &now)?;
                self.message = Some(format!("Producto actualizado: {}", product.name.trim()));
                id.clone()
            } else {
                let id = create_product(&mut app.conn, &product, &user.id, &now)?;
                self.message = Some(format!("Producto creado: {}", product.name.trim()));
                id
            };
            self.editing = None; // La transacción ya se confirmó.
            app.dialog_error = None;
            app.reload_catalog()?;
            self.filter = ProductFilter { barcode: product.code, ..Default::default() };
            self.results = self.filter.results(&app.catalog)?;
            self.selected = Some(id);
            self.generation += 1;
            ui.ctx().request_repaint();
        }
        Ok(())
    }
}

fn product_details(ui: &mut Ui, p: &Product, catalog: &Catalog) {
    let net = p.unit_price_net(catalog.price_mode);
    let gross = if catalog.price_mode == PriceMode::TaxIncluded { p.list_price }
        else { Money::from_decimal(net.as_decimal() * p.tax_rate.multiplier()) };
    let category = catalog.categories.iter().find(|c| Some(&c.id) == p.category_id.as_ref())
        .map_or("", |c| c.name.trim());
    let tax = catalog.tax(p.tax_category_id.as_str()).map_or("—", |t| t.name.as_str());
    let width = (ui.available_width() - 156.0).clamp(140.0, 495.0);
    Grid::new("product-readonly").spacing([10.0, 4.0]).show(ui, |ui| {
        for (label, value) in [
            ("Referencia", p.reference.trim().to_owned()), ("Código de barras", p.code.trim().to_owned()),
            ("Nombre", p.name.trim().to_owned()), ("Categoría", category.to_owned()), ("Impuesto", tax.to_owned()),
            ("Precio con IVA", gross.format_cop_exact()), ("Precio sin IVA", net.format_cop_exact()),
            ("Costo (COP)", p.cost.format_cop_exact()),
            ("Unidad de venta", if p.unit.is_weighted() { "Kilogramo" } else { "Unidad" }.into()),
            ("Catálogo", if catalog.in_grid(&p.id) { "Visible en ventas" } else { "Oculto en ventas" }.into()),
        ] {
            ui.label(label);
            classic::panel(classic::DESK).inner_margin(egui::Margin::symmetric(5.0, 2.0)).show(ui, |ui| {
                ui.add_sized([width, 22.0], egui::Label::new(if value.is_empty() { "—" } else { &value }).truncate())
                    .on_hover_text(&value);
            });
            ui.end_row();
        }
    });
}

fn categories(ui: &mut Ui, id: &'static str, value: &mut Option<CategoryId>, catalog: &Catalog, empty: &str, width: f32) {
    let label = |name: &str| if name.trim().is_empty() { "(Sin nombre)".to_owned() } else { name.trim().to_owned() };
    let current = value.as_ref().and_then(|id| catalog.categories.iter().find(|c| &c.id == id));
    ComboBox::from_id_salt(id).width(width).selected_text(current.map_or_else(|| empty.to_owned(), |c| label(&c.name)))
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, empty);
            for category in &catalog.categories {
                ui.selectable_value(value, Some(category.id.clone()), label(&category.name));
            }
        });
}

fn list(
    ui: &mut Ui, catalog: &Catalog, ids: &[ProductId], selected: &mut Option<ProductId>, height: f32,
    generation: u64, scroll_to: Option<usize>,
) -> bool {
    let mut accepted = false;
    classic::panel(classic::DESK).show(ui, |ui| {
        if ids.is_empty() { ui.weak("No se encontraron productos."); }
        let mut scroll = ScrollArea::vertical().id_salt(("filtered-products", generation)).auto_shrink([false, false])
            .max_height(height);
        if let Some(index) = scroll_to {
            scroll = scroll.vertical_scroll_offset(index as f32 * (28.0 + ui.spacing().item_spacing.y));
        }
        scroll.show_rows(ui, 28.0, ids.len(), |ui, range| {
                for id in &ids[range] {
                    if let Some(p) = catalog.product_by_id(id.as_str()) {
                        ui.push_id(id.as_str(), |ui| {
                            let label = format!("{} · {}", p.code.trim(), p.name.trim());
                            let response = ui.add_sized([ui.available_width(), 28.0],
                                Button::new(label).truncate().selected(selected.as_ref() == Some(id)));
                            if response.clicked() || response.double_clicked() {
                                *selected = Some(id.clone());
                                accepted |= response.double_clicked();
                            }
                        });
                    }
                }
            });
    });
    accepted
}

pub(super) fn primary(text: &str) -> Button<'_> {
    Button::new(RichText::new(text).strong().color(Color32::WHITE))
        .fill(Color32::from_rgb(64, 129, 188)).min_size(egui::vec2(124.0, 36.0))
}

pub(super) fn numeric_pad(ui: &mut Ui, width: f32, value: &mut String, decimal: bool, max: usize) -> bool {
    let each = (width - 2.0 * ui.spacing().item_spacing.x) / 3.0;
    let mut pressed = None;
    for row in [&["CE", "⌫"][..], &["1", "2", "3"], &["4", "5", "6"], &["7", "8", "9"], &["0", ","]] {
        ui.horizontal(|ui| {
            for (index, &key) in row.iter().enumerate() {
                let w = if row.len() == 2 && index == 0 { 2.0 * each + ui.spacing().item_spacing.x } else { each };
                if ui.add_enabled(key != "," || decimal, Button::new(RichText::new(key).size(22.0))
                    .min_size(egui::vec2(w, 37.0))).clicked()
                {
                    pressed = Some(key);
                }
            }
        });
    }
    if let Some(key) = pressed {
        match key {
            "CE" => value.clear(),
            "⌫" => { value.pop(); }
            "," if value.contains(['.', ',']) => {}
            "," if value.is_empty() => value.push_str("0,"),
            _ if value.chars().count() < max => value.push_str(key),
            _ => {}
        }
    }
    pressed.is_some()
}
