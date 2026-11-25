use super::*;
use eframe::egui::{Event, FullOutput, Modifiers, PointerButton, Pos2, RawInput, Rect, Shape, Vec2};
use pos_core::{Payment, PaymentMethod};

fn fixture() -> (tempfile::TempDir, Context, App) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("pos.db");
    let conn = Connection::open(&db).unwrap();
    pos_store::apply_schema(&conn).unwrap();
    conn.execute_batch("
        INSERT INTO people (ID, NAME, ROLE, VISIBLE, APPPASSWORD)
            VALUES ('0','Administrador','0',1,'plain:123');
         INSERT INTO people (ID, NAME, ROLE, VISIBLE, APPPASSWORD)
            VALUES ('2','Empleado','2',1,NULL);
         INSERT INTO locations (ID, NAME) VALUES ('0','General');
        INSERT INTO taxcategories (ID, NAME) VALUES ('100','Sin IVA');
        INSERT INTO taxes (ID, NAME, CATEGORY, RATE) VALUES ('100','Sin IVA','100','0');
        INSERT INTO categories (ID, NAME) VALUES ('c','Categoría');
        INSERT INTO products (ID, REFERENCE, CODE, NAME, PRICEBUY, PRICESELL, CATEGORY, TAXCAT, ISSCALE)
            VALUES ('unit','ref','123456','Café','600','1000','c','100',0),
                   ('kg','ref2','987654','Granos','600','4000','c','100',1),
                   ('hidden','ref3','564738','Habichuela','600','3000','c','100',0);
        INSERT INTO products_cat (PRODUCT, CATORDER) VALUES ('unit',5),('kg',0);
        INSERT INTO ticketsnum (ID) VALUES (23005);
        INSERT INTO pos_meta (KEY, VALUE) VALUES ('schema_version','1');
    ").unwrap();
    let catalog = pos_store::load_catalog(&conn, PriceMode::TaxIncluded).unwrap();
    let settings = Settings::load(&conn, &LaunchOptions {
        db: db.clone(), printer: None, paper_mm: None,
    }).unwrap();
    let ctx = Context::default();
    ctx.set_style(crate::classic::style());
    let printer = Printer::start(ctx.clone()).unwrap();
    let mut app = App::new(conn, catalog, db, settings, printer);
    assert!(app.login("no-existe").is_err());
    app.login("0").unwrap();
    assert!(matches!(app.dialog, Some(Dialog::Opening(_))));
    app.open_session(Money::ZERO).unwrap();
    (dir, ctx, app)
}

fn frame(ctx: &Context, app: &mut App, events: Vec<Event>) {
    render(ctx, app, egui::vec2(1100.0, 730.0), events);
}

fn render(ctx: &Context, app: &mut App, size: Vec2, events: Vec<Event>) -> FullOutput {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
        events,
        ..Default::default()
    };
    ctx.run(input, |ctx| {
        if app.user.is_some() { app.render_sales(ctx); } else { app.render_login(ctx); }
        app.render_dialog(ctx);
    })
}

fn text_rect(output: &FullOutput, label: &str) -> Rect {
    fn find(shape: &Shape, label: &str) -> Option<Rect> {
        match shape {
            Shape::Text(text) if text.galley.text() == label => Some(text.visual_bounding_rect()),
            Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, label)),
            _ => None,
        }
    }
    output.shapes.iter().rev().find_map(|item| {
        find(&item.shape, label).map(|rect| rect.intersect(item.clip_rect)).filter(|rect| rect.is_positive())
    }).unwrap_or_else(|| panic!("no se ve el control {label:?}"))
}

fn click(ctx: &Context, app: &mut App, size: Vec2, at: Pos2) {
    let button = |pressed| Event::PointerButton {
        pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE,
    };
    render(ctx, app, size, vec![Event::PointerMoved(at), button(true)]);
    render(ctx, app, size, vec![button(false)]);
}

fn key(key: Key, pressed: bool) -> Event {
    Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }
}

#[test]
fn escaneo_hid_peso_y_dialogo_de_cobro() {
    let (_dir, ctx, mut app) = fixture();
    assert_eq!(app.visible_products.len(), 2);
    assert_eq!(app.catalog.products[app.visible_products[0]].id.as_str(), "kg");
    assert_eq!(app.catalog.search("habichuela", 10).len(), 1);
    assert_eq!(app.catalog.search("cafe", 10).len(), 1);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![Event::Text("12345".into()), key(Key::Enter, true)]);
    assert!(app.draft.is_empty(), "no debe vender una coincidencia parcial de código");
    frame(&ctx, &mut app, vec![key(Key::Enter, false)]);
    frame(&ctx, &mut app, vec![Event::Text("123456".into()), key(Key::Enter, true)]);
    assert_eq!(app.draft.line_count(), 1);
    frame(&ctx, &mut app, vec![key(Key::Enter, false)]);
    frame(&ctx, &mut app, vec![Event::Text("987654".into()), key(Key::Enter, true)]);
    assert!(matches!(app.dialog, Some(Dialog::Edit(EditKind::Weight(_), _))));
    frame(&ctx, &mut app, vec![key(Key::Enter, false)]);
    frame(&ctx, &mut app, vec![Event::Text("0,850".into()), key(Key::Enter, true)]);
    assert_eq!(app.draft.line_count(), 2);
    assert_eq!(app.draft.totals().unwrap().total, Money::from_pesos(4400));
    frame(&ctx, &mut app, vec![key(Key::Enter, false), key(Key::F6, true)]);
    assert!(matches!(app.dialog, Some(Dialog::Checkout(_))));
}

#[test]
fn layout_clasico_visible_en_ventana_compacta_y_cobro_tactil() {
    let (_dir, ctx, mut app) = fixture();
    let size = egui::vec2(820.0, 600.0);
    render(&ctx, &mut app, size, vec![]);
    let output = render(&ctx, &mut app, size, vec![]);
    let categories = text_rect(&output, "Categorías");
    let products = text_rect(&output, "Productos · Todas");
    let ticket = text_rect(&output, "Artículo");
    let last = text_rect(&output, "     Último pago");
    let subtotal = text_rect(&output, "Subtotal");
    let total = text_rect(&output, "Total");
    let keypad = text_rect(&output, "CE");
    let scanner = text_rect(&output, "Código de barras");
    let status = text_rect(&output, "Espacio / F6 Cobrar  ·  + / − Unidades  ·  F1 Lector  ·  F2 Buscar  ·  F8 Último pago");
    assert!(categories.right() < products.left());
    assert!(last.bottom() < ticket.top());
    assert!(subtotal.right() < total.left() && total.bottom() < products.top());
    assert!(total.right() < scanner.left() && scanner.bottom() < keypad.top());
    assert!(keypad.top() < products.top() && products.bottom() < status.top());

    click(&ctx, &mut app, size, text_rect(&output, "Café").center());
    assert_eq!(app.draft.line_count(), 1);
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "+").center());
    assert_eq!(app.draft.totals().unwrap().total, Money::from_pesos(2000));
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "=").center());
    assert!(matches!(app.dialog, Some(Dialog::Checkout(_))));

    let output = render(&ctx, &mut app, size, vec![]);
    let method = text_rect(&output, "     Nequi");
    let denomination = text_rect(&output, "5.000");
    let pad = text_rect(&output, "CE");
    assert!(method.right() < denomination.left() && denomination.right() < pad.left());
    click(&ctx, &mut app, size, denomination.center());
    let output = render(&ctx, &mut app, size, vec![]);
    assert!(text_rect(&output, "$ 3.000").is_positive());
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Confirmar venta").center());
    let sale = pos_store::last_sale(&app.conn).unwrap().unwrap();
    assert_eq!(sale.total, Money::from_pesos(2000));
    assert_eq!(sale.change, Money::from_pesos(3000));
    assert!(matches!(app.dialog, Some(Dialog::Change { amount, .. }) if amount == Money::from_pesos(3000)));
    render(&ctx, &mut app, size, vec![]);
    let output = render(&ctx, &mut app, size, vec![]);
    assert!(text_rect(&output, "Vueltas").is_positive());
    assert!(text_rect(&output, "$ 3.000").is_positive());
    assert!(app.draft.is_empty() && !app.has_pending_sales());
    click(&ctx, &mut app, size, text_rect(&output, "Continuar").center());
    assert!(app.dialog.is_none());
}

#[test]
fn espacio_abre_cobro_y_signos_actualizan_una_vez_sin_editar_el_escaner() {
    let (_dir, ctx, mut app) = fixture();
    app.add_product("unit".into());
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::Plus, true), Event::Text("+".into())]);
    assert_eq!(app.draft.lines()[0].quantity(), Qty::from_units(2));
    assert!(app.search.is_empty());
    frame(&ctx, &mut app, vec![key(Key::Plus, false), key(Key::Minus, true), Event::Text("-".into())]);
    assert_eq!(app.draft.lines()[0].quantity(), Qty::one());
    assert!(app.search.is_empty());
    frame(&ctx, &mut app, vec![key(Key::Minus, false), Event::Text("cafe".into())]);
    frame(&ctx, &mut app, vec![key(Key::Space, true), Event::Text(" ".into())]);
    assert_eq!(app.search, "cafe ");
    assert!(app.dialog.is_none());
    frame(&ctx, &mut app, vec![key(Key::Space, false), key(Key::Escape, true)]);
    frame(&ctx, &mut app, vec![key(Key::Escape, false), key(Key::Space, true), Event::Text(" ".into())]);
    assert!(matches!(app.dialog, Some(Dialog::Checkout(_))));
    frame(&ctx, &mut app, vec![key(Key::Space, false)]);
    frame(&ctx, &mut app, vec![Event::Text("2000".into()), key(Key::Enter, true)]);
    assert!(pos_store::last_sale(&app.conn).unwrap().is_none());
    frame(&ctx, &mut app, vec![key(Key::Enter, false)]);
    frame(&ctx, &mut app, vec![key(Key::Enter, true)]);
    let sale = pos_store::last_sale(&app.conn).unwrap().unwrap();
    assert_eq!(sale.change, Money::from_pesos(1000));
    frame(&ctx, &mut app, vec![key(Key::Enter, false)]);
    frame(&ctx, &mut app, vec![key(Key::Enter, true)]);
    assert!(app.dialog.is_none());
    assert_eq!(app.conn.query_row("SELECT COUNT(*) FROM tickets", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn ventas_abiertas_se_pueden_retomar_y_cobrar_por_separado() {
    let (_dir, ctx, mut app) = fixture();
    let size = egui::vec2(820.0, 600.0);
    app.add_product("unit".into());
    app.draft.set_note(app.selected_line.unwrap(), Some("Llevar")).unwrap();
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Nueva venta").center());
    assert!(app.draft.is_empty());
    assert_eq!(app.parked_sales.len(), 1);
    assert!(app.prepare_close().is_err());
    assert!(app.logout().is_err());
    assert!(pos_store::last_sale(&app.conn).unwrap().is_none());
    app.add_product("hidden".into());
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Ventas abiertas (2)").center());
    let output = render(&ctx, &mut app, size, vec![]);
    let time = app.parked_sales[0].opened_at.get(11..16).unwrap();
    let label = format!("#1 · Administrador · {time} · $ 1.000");
    click(&ctx, &mut app, size, text_rect(&output, &label).center());
    assert_eq!(app.sale_number, 1);
    assert_eq!(app.draft.lines()[0].note(), Some("Llevar"));
    assert_eq!(app.selected_line, Some(app.draft.lines()[0].key()));
    let mut payments = Payments::new();
    payments.add(Payment::cash(Money::from_pesos(1000), Money::from_pesos(1000)).unwrap());
    app.commit_sale(&payments).unwrap();
    assert_eq!(app.parked_sales.len(), 1);
    app.resume_sale(2);
    assert_eq!(app.draft.totals().unwrap().total, Money::from_pesos(3000));
    assert_eq!(pos_store::last_sale(&app.conn).unwrap().unwrap().total, Money::from_pesos(1000));
    let mut payments = Payments::new();
    payments.add(Payment::electronic(pos_core::PaymentMethod::Nequi, Money::from_pesos(3000), None).unwrap());
    app.commit_sale(&payments).unwrap();
    let last_sale = pos_store::last_sale(&app.conn).unwrap().unwrap();
    assert_eq!(last_sale.number, 23007);
    assert_eq!(last_sale.total, Money::from_pesos(3000));
    assert!(!app.has_pending_sales());
}

#[test]
fn buscar_por_nombre_agrega_y_editar_linea_muestra_importes_y_teclado() {
    let (_dir, ctx, mut app) = fixture();
    let size = egui::vec2(820.0, 600.0);
    render(&ctx, &mut app, size, vec![]);
    render(&ctx, &mut app, size, vec![key(Key::F2, true)]);
    render(&ctx, &mut app, size, vec![key(Key::F2, false), Event::Text("cafe".into()), key(Key::Enter, true)]);
    let output = render(&ctx, &mut app, size, vec![key(Key::Enter, false)]);
    let result = text_rect(&output, "123456 · Café");
    let add = text_rect(&output, "Agregar a la venta");
    assert!(result.bottom() < add.top());
    click(&ctx, &mut app, size, add.center());
    assert_eq!(app.draft.line_count(), 1);
    assert_eq!(app.draft.lines()[0].product().name, "Café");

    render(&ctx, &mut app, size, vec![key(Key::F3, true)]);
    let output = render(&ctx, &mut app, size, vec![key(Key::F3, false)]);
    assert!(text_rect(&output, "Editar línea").is_positive());
    assert!(text_rect(&output, "Precio con IVA").right() < text_rect(&output, "CE").left());
    click(&ctx, &mut app, size, text_rect(&output, "CE").center());
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "3").center());
    let output = render(&ctx, &mut app, size, vec![]);
    assert!(text_rect(&output, "$ 3.000").is_positive());
    click(&ctx, &mut app, size, text_rect(&output, "Aceptar").center());
    assert_eq!(app.draft.lines()[0].quantity(), Qty::from_units(3));
}

#[test]
fn usuarios_entran_con_un_clic_y_conservan_su_rol() {
    let (_dir, ctx, mut app) = fixture();
    let size = egui::vec2(820.0, 600.0);
    app.logout().unwrap();
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Administrador").center());
    assert_eq!(app.user.as_ref().unwrap().id.as_str(), "0");
    assert!(app.can(Capability::ChangePrice));
    app.logout().unwrap();
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Empleado").center());
    assert_eq!(app.user.as_ref().unwrap().id.as_str(), "2");
    assert!(!app.can(Capability::ChangePrice));
}

#[test]
fn editar_precio_con_teclado_tactil_no_cambia_el_catalogo() {
    let (_dir, ctx, mut app) = fixture();
    let size = egui::vec2(820.0, 600.0);
    app.add_product("unit".into());
    render(&ctx, &mut app, size, vec![key(Key::F3, true)]);
    let output = render(&ctx, &mut app, size, vec![key(Key::F3, false)]);
    click(&ctx, &mut app, size, text_rect(&output, "1000").center());
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "CE").center());
    for digit in ["1", "5", "0", "0"] {
        let output = render(&ctx, &mut app, size, vec![]);
        // El botón del teclado está a la derecha de los campos.
        let key = text_rect(&output, digit);
        assert!(key.left() > 425.0, "{digit}: {key:?}");
        click(&ctx, &mut app, size, key.center());
    }
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Aceptar").center());
    assert_eq!(app.draft.totals().unwrap().total, Money::from_pesos(1500), "{:?}", app.dialog_error);
    assert_eq!(app.catalog.product_by_code("123456").unwrap().list_price, Money::from_pesos(1000));
}

#[test]
fn confirmacion_eliminar_no_toca_la_venta_hasta_el_si() {
    let (_dir, ctx, mut app) = fixture();
    let size = egui::vec2(820.0, 600.0);
    app.add_product("unit".into());
    for (caption, expected) in [("No", 1), ("Sí", 0)] {
        render(&ctx, &mut app, size, vec![key(Key::Delete, true)]);
        let output = render(&ctx, &mut app, size, vec![key(Key::Delete, false)]);
        assert!(text_rect(&output, "Eliminar línea").is_positive());
        assert!(text_rect(&output, "Sí").left() < text_rect(&output, "No").left());
        click(&ctx, &mut app, size, text_rect(&output, caption).center());
        assert_eq!(app.draft.line_count(), expected);
    }
    assert_eq!(app.conn.query_row("SELECT COUNT(*) FROM lineremoved", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn stock_products_busca_crea_y_deja_el_producto_disponible_en_ventas() {
    let (_dir, ctx, mut app) = fixture();
    let size = egui::vec2(1100.0, 730.0);
    render(&ctx, &mut app, size, vec![key(Key::F10, true)]);
    let output = render(&ctx, &mut app, size, vec![key(Key::F10, false)]);
    click(&ctx, &mut app, size, text_rect(&output, "Stock").center());
    let output = render(&ctx, &mut app, size, vec![]);
    let products = text_rect(&output, "     Products");
    assert!(text_rect(&output, "Manage").bottom() < products.top());
    click(&ctx, &mut app, size, products.center());
    render(&ctx, &mut app, size, vec![]);
    render(&ctx, &mut app, size, vec![Event::Text("cafe".into()), key(Key::Enter, true)]);
    let output = render(&ctx, &mut app, size, vec![key(Key::Enter, false)]);
    assert!(text_rect(&output, "1 productos").is_positive());
    assert!(text_rect(&output, "123456 · Café").is_positive());

    click(&ctx, &mut app, size, text_rect(&output, "+ Nuevo producto").center());
    render(&ctx, &mut app, size, vec![]);
    for value in ["ARROZ-X", "7701234567000", "Arroz nuevo"] {
        render(&ctx, &mut app, size, vec![Event::Text(value.into())]);
        render(&ctx, &mut app, size, vec![key(Key::Tab, true)]);
        render(&ctx, &mut app, size, vec![key(Key::Tab, false)]);
    }
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Seleccionar").center());
    let output = render(&ctx, &mut app, size, vec![]);
    // La opción queda debajo del control de categoría.
    let choice = text_rect(&output, "Categoría");
    click(&ctx, &mut app, size, choice.center());
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "COP").center());
    render(&ctx, &mut app, size, vec![Event::Text("6400,50".into())]);
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Guardar producto").center());
    let result = app.catalog.product_by_code("7701234567000")
        .unwrap_or_else(|| panic!("no se creó el producto: {:?}", app.dialog_error));
    assert_eq!(result.name, "Arroz nuevo");
    assert_eq!(result.reference, "ARROZ-X");
    assert_eq!(result.list_price, Money::from_cents(640050));
    assert_eq!(result.tax_category_id.as_str(), "100");
    assert!(result.stock.is_none());
    assert!(app.visible_products.iter().any(|&index| app.catalog.products[index].id == result.id));
    let id = result.id.clone();
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Editar").center());
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "6400.5").center());
    let ctrl = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
    let select_all = Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: ctrl };
    render(&ctx, &mut app, size, vec![select_all, Event::Text("7200,25".into())]);
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Guardar producto").center());
    let result = app.catalog.product_by_id(id.as_str()).unwrap();
    assert_eq!(result.list_price, Money::from_cents(720025), "{:?}", app.dialog_error);
    assert_eq!(result.code, "7701234567000");
}

#[test]
fn products_muestra_una_lista_alta_y_el_primer_registro_en_tamano_compacto() {
    let (_dir, ctx, mut app) = fixture();
    for i in 0..30 {
        app.conn.execute(
            "INSERT INTO products (ID,REFERENCE,CODE,NAME,CATEGORY,TAXCAT,PRICEBUY,PRICESELL,ISSCALE)
             VALUES (?1,?1,?1,?1,'c','100','1','2',0)", [format!("A{i:02}")],
        ).unwrap();
    }
    app.reload_catalog().unwrap();
    let form = products::ProductsForm::new(&app.catalog);
    app.show_dialog(Dialog::Menu(Box::new(menu::MenuDialog::Products(Box::new(form)))));
    for size in [egui::vec2(820.0, 600.0), egui::vec2(1100.0, 730.0)] {
        render(&ctx, &mut app, size, vec![]);
        let output = render(&ctx, &mut app, size, vec![]);
        let first = text_rect(&output, "A00 · A00");
        let eighth = text_rect(&output, "A07 · A07");
        let detail = text_rect(&output, "A00");
        assert!(first.bottom() < eighth.top());
        assert!(first.right() < detail.left());
    }
}

#[test]
fn cierre_muestra_impuestos_y_conserva_el_arqueo_despues_de_cerrar() {
    let (_dir, ctx, mut app) = fixture();
    app.add_product("unit".into());
    let mut payments = Payments::new();
    payments.add(Payment::cash(Money::from_pesos(1000), Money::from_pesos(1000)).unwrap());
    app.commit_sale(&payments).unwrap();
    app.prepare_close().unwrap();
    let size = egui::vec2(820.0, 600.0);
    let output = render(&ctx, &mut app, size, vec![]);
    assert!(text_rect(&output, "Medio de pago").right() < text_rect(&output, "Transacciones").left());
    assert!(text_rect(&output, "Sin IVA").is_positive());
    assert!(text_rect(&output, "Confirmar cierre").is_positive());
    app.finish_close(Money::from_pesos(900)).unwrap();
    app.show_dialog(Dialog::Menu(Box::new(menu::MenuDialog::ClosedCash)));
    let output = render(&ctx, &mut app, size, vec![]);
    assert!(text_rect(&output, "Diferencia").is_positive());
    assert!(text_rect(&output, "Imprimir cierre").is_positive());
    let closed = app.last_closed.as_ref().unwrap();
    assert_eq!(closed.taxes[0].base, Money::from_pesos(1000));
    assert_eq!(closed.close.difference, Money::from_pesos(-100));
}

#[test]
fn tools_previsualiza_importacion_y_no_guarda_hasta_confirmar() {
    let (dir, ctx, mut app) = fixture();
    let csv = dir.path().join("precios.csv");
    std::fs::write(&csv, "accion;codigo;precio\nprecio;123456;1600\n").unwrap();
    let size = egui::vec2(1100.0, 730.0);
    render(&ctx, &mut app, size, vec![key(Key::F10, true)]);
    let output = render(&ctx, &mut app, size, vec![key(Key::F10, false)]);
    click(&ctx, &mut app, size, text_rect(&output, "Tools").center());
    let output = render(&ctx, &mut app, size, vec![]);
    assert!(text_rect(&output, "CSV Import").is_positive());
    assert!(text_rect(&output, "CSV Import Reports").is_positive());
    click(&ctx, &mut app, size, text_rect(&output, "     Products CSV File").center());
    render(&ctx, &mut app, size, vec![]);
    render(&ctx, &mut app, size, vec![Event::Text(csv.to_string_lossy().into_owned())]);
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Previsualizar CSV").center());
    assert_eq!(app.catalog.product_by_code("123456").unwrap().list_price, Money::from_pesos(1000));
    let output = render(&ctx, &mut app, size, vec![]);
    assert!(text_rect(&output, "Vista previa: 1 de 1 filas aplicables. No se guardó nada.").is_positive());
    click(&ctx, &mut app, size, text_rect(&output, "Aplicar filas válidas y registrar el resultado").center());
    assert_eq!(app.catalog.product_by_code("123456").unwrap().list_price, Money::from_pesos(1600));
}

#[test]
fn menu_lateral_agrupa_las_secciones_y_respeta_los_permisos() {
    let (_dir, ctx, mut app) = fixture();
    let size = egui::vec2(820.0, 600.0);
    render(&ctx, &mut app, size, vec![key(Key::F10, true)]);
    let output = render(&ctx, &mut app, size, vec![key(Key::F10, false)]);
    assert!(app.menu_open);
    let mut last = 0.0;
    for label in ["Register", "Sales", "Edit Sales", "Payments", "Close Cash", "Administration", "Stock",
        "Maintenance", "Utilities", "Tools", "Printers", "System", "Configuration", "Logout"]
    {
        let rect = text_rect(&output, label);
        assert!(rect.top() > last && rect.right() < 180.0, "{label}: {rect:?}, last {last}");
        last = rect.top();
    }
    click(&ctx, &mut app, size, text_rect(&output, "Payments").center());
    assert!(matches!(app.dialog, Some(Dialog::Cash(CashDirection::In, _, _))));
    assert!(!app.menu_open);
    let output = render(&ctx, &mut app, size, vec![]);
    assert!(text_rect(&output, "Payments").left() < 100.0);
    assert!(text_rect(&output, "CE").left() > text_rect(&output, "Motivo").right());
    click(&ctx, &mut app, size, text_rect(&output, "Salida de caja").center());
    assert!(matches!(app.dialog, Some(Dialog::Cash(CashDirection::Out, _, _))));
    render(&ctx, &mut app, size, vec![key(Key::F10, true)]);
    let output = render(&ctx, &mut app, size, vec![key(Key::F10, false)]);
    click(&ctx, &mut app, size, text_rect(&output, "Sales").center());
    assert!(app.dialog.is_none());
    assert!(app.draft.is_empty());
    assert_eq!(app.conn.query_row("SELECT COUNT(*) FROM payments", [], |r| r.get::<_, i64>(0)).unwrap(), 0);

    app.user.as_mut().unwrap().role = pos_core::RoleKind::Employee;
    app.menu_open = true;
    let output = render(&ctx, &mut app, size, vec![]);
    for disabled in ["Payments", "Close Cash", "Stock", "Maintenance", "Tools", "Configuration", "Printers"] {
        assert!(!output.shapes.iter().any(|item| matches!(&item.shape,
            Shape::Text(text) if text.galley.text() == disabled)), "{disabled} visible sin permiso");
    }
    assert!(text_rect(&output, "Edit Sales").is_positive());
    assert!(text_rect(&output, "Logout").is_positive());
}

#[test]
fn utilities_printers_configura_recibo_e_imprime_muestra_y_venta_sin_pulso() {
    let (dir, ctx, mut app) = fixture();
    let size = egui::vec2(820.0, 600.0);
    render(&ctx, &mut app, size, vec![key(Key::F10, true)]);
    let output = render(&ctx, &mut app, size, vec![key(Key::F10, false)]);
    click(&ctx, &mut app, size, text_rect(&output, "Printers").center());

    let output = render(&ctx, &mut app, size, vec![]);
    let destination_label = text_rect(&output, "Destino (vacío = guardar a archivo)");
    click(&ctx, &mut app, size, destination_label.left_bottom() + egui::vec2(15.0, 13.0));
    let destination = dir.path().join("salida");
    render(&ctx, &mut app, size, vec![Event::Text(format!("file:{}", destination.display()))]);
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "58 mm").center());
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Parcial").center());
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Guardar impresora").center());
    assert_eq!(app.settings.print_options.paper, Paper::Mm58);
    assert_eq!(app.settings.print_options.cut, Cut::Partial);
    assert!(matches!(&app.settings.destination, Destination::Files(dir) if dir == &destination));

    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Contenido del recibo").center());
    let output = render(&ctx, &mut app, size, vec![]);
    assert!(text_rect(&output, "Encabezado y pie").right() < text_rect(&output, "Vista previa del recibo").left());
    let name = text_rect(&output, "Nombre del negocio");
    click(&ctx, &mut app, size, name.left_bottom() + egui::vec2(15.0, 13.0));
    render(&ctx, &mut app, size, vec![Event::Text("Mi tienda".into())]);

    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Nombre del cajero").center());
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Código de barras").center());
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Imprimir muestra").center());
    let sample = app.printer.done.recv_timeout(Duration::from_secs(3)).unwrap();
    let sample_text = String::from_utf8_lossy(&sample.job.bytes);
    assert!(sample.result.is_ok() && sample_text.contains("MUESTRA - NO ES UNA VENTA"));
    assert!(sample_text.contains("Mi tienda") && !sample_text.contains("Cajero de ejemplo"), "{sample_text}");
    assert!(sample_text.contains("1234567890123"));
    assert_eq!(sample.job.audit_key, "printer:receipt-sample");
    assert!(sample.job.bytes.ends_with(&[29, b'V', 1]));
    assert!(!sample.job.bytes.windows(2).any(|s| s == b"\x1bp"));
    assert!(pos_store::last_sale(&app.conn).unwrap().is_none());
    assert!(app.settings.business.name.is_empty());
    app.process_print(sample);

    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Guardar contenido").center());
    assert_eq!(app.settings.business.name, "Mi tienda");
    let mut options = LaunchOptions { db: app.db.clone(), paper_mm: None, printer: None };
    let stored = Settings::load(&app.conn, &options).unwrap();
    assert_eq!(stored.business.name, "Mi tienda");
    assert_eq!(stored.receipt_layout, app.settings.receipt_layout);
    assert!(!stored.receipt_layout.cashier && stored.receipt_layout.product_code);
    assert_eq!(stored.print_options.paper, Paper::Mm58);
    assert_eq!(stored.print_options.cut, Cut::Partial);
    assert!(matches!(stored.destination, Destination::Files(dir) if dir == destination));

    options.paper_mm = Some(80);
    options.printer = Some("tcp:localhost:9100".into());
    let overridden = Settings::load(&app.conn, &options).unwrap();
    assert_eq!(overridden.print_options.paper, Paper::Mm80);
    assert_eq!(overridden.print_options.cut, Cut::Partial);
    assert!(matches!(overridden.destination, Destination::Tcp(address) if address == "localhost:9100"));

    app.hide_dialog();
    app.add_product("unit".into());
    let mut payments = Payments::new();
    payments.add(Payment::cash(Money::from_pesos(1000), Money::from_pesos(1000)).unwrap());
    app.commit_sale(&payments).unwrap();
    assert!(app.printer.done.try_recv().is_err());
    frame(&ctx, &mut app, vec![key(Key::F8, true)]);
    let sale = app.printer.done.recv_timeout(Duration::from_secs(3)).unwrap();
    let sale_text = String::from_utf8_lossy(&sale.job.bytes);
    assert!(sale.result.is_ok() && sale_text.contains("Mi tienda") && sale_text.contains("Ticket 23006"), "{sale_text}");
    assert!(sale_text.contains("123456") && !sale_text.contains("Administrador") && !sale_text.contains("MUESTRA"));
    assert!(!sale.job.bytes.windows(2).any(|s| s == b"\x1bp"));
    assert_eq!(std::fs::read_dir(destination).unwrap().count(), 2);
    assert!(!dir.path().join("tirillas").exists());
}

#[test]
fn ventas_reproduce_la_distribucion_de_la_captura_a_1920_por_1080() {
    let (_dir, ctx, mut app) = fixture();
    let size = egui::vec2(1920.0, 1080.0);
    let output = render(&ctx, &mut app, size, vec![]);
    let ticket = text_rect(&output, "Artículo");
    let catalog = text_rect(&output, "Productos · Todas");
    let keys = text_rect(&output, "CE");
    assert!(ticket.top() < 140.0 && (200.0..400.0).contains(&ticket.left()),
        "ticket={ticket:?}; catálogo={catalog:?}; teclado={keys:?}");
    assert!(catalog.top() > 680.0 && catalog.top() < 790.0, "catálogo={catalog:?}");
    assert!(keys.left() > 1600.0 && keys.top() < 240.0, "teclado={keys:?}");
    assert!(text_rect(&output, "Impresora").right() < keys.left());
}

#[test]
fn guardar_no_imprime_f8_si_y_repetir_marca_reimpresion() {
    let (dir, ctx, mut app) = fixture();
    app.add_product("unit".into());
    let mut payments = Payments::new();
    payments.add(Payment::cash(Money::from_pesos(1000), Money::from_pesos(2000)).unwrap());
    app.commit_sale(&payments).unwrap();
    assert!(app.draft.is_empty());
    assert!(!dir.path().join("tirillas").exists());
    let sale = pos_store::last_sale(&app.conn).unwrap().unwrap();
    assert_eq!(sale.number, 23006);
    assert_eq!(sale.change, Money::from_pesos(1000));

    frame(&ctx, &mut app, vec![key(Key::F8, true)]);
    for reprint in [false, true] {
        if reprint {
            frame(&ctx, &mut app, vec![key(Key::F8, false)]);
            frame(&ctx, &mut app, vec![key(Key::F8, true)]);
        }
        let outcome = app.printer.done.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(outcome.result.is_ok());
        assert!(outcome.job.receipt);
        assert_eq!(outcome.job.bytes.windows(6).any(|part| part == b"REIMPR"), reprint);
        app.process_print(outcome);
    }
    assert_eq!(std::fs::read_dir(dir.path().join("tirillas")).unwrap().count(), 2);
    let count: i64 = app.conn.query_row(
        "SELECT COUNT(*) FROM pos_audit WHERE ACTION = 'PRINT_SENT'", [], |r| r.get(0),
    ).unwrap();
    assert_eq!(count, 2);
}

#[test]
fn pago_confirmado_envia_solo_pulso_y_f8_envia_solo_tirilla() {
    use std::io::Read;
    let (_dir, ctx, mut app) = fixture();
    let network = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let receipts = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    app.settings.destination = Destination::Tcp(receipts.local_addr().unwrap().to_string());
    app.settings.drawer_destination = Some(Destination::Tcp(network.local_addr().unwrap().to_string()));
    app.settings.drawer = true;
    app.add_product("unit".into());
    assert!(app.commit_sale(&Payments::new()).is_err());
    assert!(app.printer.done.try_recv().is_err());

    let mut payments = Payments::new();
    payments.add(Payment::electronic(PaymentMethod::Nequi, Money::from_pesos(1000), None).unwrap());
    app.commit_sale(&payments).unwrap();
    let size = egui::vec2(1100.0, 730.0);
    render(&ctx, &mut app, size, vec![]);
    let output = render(&ctx, &mut app, size, vec![key(Key::F8, true)]);
    assert!(text_rect(&output, "Pago guardado. Enviando el pulso del cajón…").is_positive());
    assert!(!app.printing_receipt, "no se debe imprimir mientras se espera el pulso");
    let outcome = app.printer.done.recv_timeout(Duration::from_secs(4)).unwrap();
    assert!(outcome.result.is_ok() && !outcome.job.receipt);
    let (mut socket, _) = network.accept().unwrap();
    socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut data = Vec::new();
    socket.read_to_end(&mut data).unwrap();
    assert_eq!(data, [0x1b, b'p', 0, 25, 250]);
    app.process_print(outcome);
    assert!(app.sale_drawer.is_none());
    let (pulse, print, drawer): (i64, i64, i64) = app.conn.query_row(
        "SELECT (SELECT COUNT(*) FROM pos_audit WHERE ACTION='DRAWER_SENT'),
                (SELECT COUNT(*) FROM pos_audit WHERE ACTION='PRINT_SENT'),
                (SELECT COUNT(*) FROM draweropened WHERE TICKETID='23006')",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).unwrap();
    assert_eq!((pulse, print, drawer), (1, 0, 1));

    render(&ctx, &mut app, size, vec![key(Key::F8, false)]);
    let output = render(&ctx, &mut app, size, vec![]);
    assert!(text_rect(&output, "Vueltas").is_positive());
    frame(&ctx, &mut app, vec![key(Key::F8, true)]);
    let outcome = app.printer.done.recv_timeout(Duration::from_secs(4)).unwrap();
    assert!(outcome.result.is_ok() && outcome.job.receipt);
    let (mut socket, _) = receipts.accept().unwrap();
    socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut data = Vec::new();
    socket.read_to_end(&mut data).unwrap();
    assert!(data.len() > 5 && !data.windows(2).any(|b| b == [0x1b, b'p']));
    assert!(app.printer.done.try_recv().is_err());
}

#[test]
fn espera_del_cajon_no_bloquea_vueltas_si_el_dispositivo_tarda() {
    let (_dir, ctx, mut app) = fixture();
    app.add_product("unit".into());
    let mut payments = Payments::new();
    payments.add(Payment::cash(Money::from_pesos(1000), Money::from_pesos(2000)).unwrap());
    app.commit_sale(&payments).unwrap();
    app.sale_drawer = Some(PendingDrawer { key: "simulado".into(), until: Instant::now() });
    let size = egui::vec2(820.0, 600.0);
    render(&ctx, &mut app, size, vec![]);
    let output = render(&ctx, &mut app, size, vec![]);
    assert!(text_rect(&output, "Vueltas").is_positive());
    assert!(app.printer_notice.as_ref().unwrap().1.contains("ya quedó guardado"));
    assert!(app.sale_drawer.is_none() && app.draft.is_empty());
    assert!(app.printer.done.try_recv().is_err());
}

#[test]
fn configuration_guarda_marca_permite_probar_lector_y_verifica_sqlite() {
    let (dir, ctx, mut app) = fixture();
    image::RgbaImage::from_pixel(32, 24, image::Rgba([12, 80, 160, 255]))
        .save(dir.path().join("logo.png")).unwrap();
    std::fs::write(dir.path().join("marca.txt"), "Mercado de prueba").unwrap();
    let size = egui::vec2(1100.0, 850.0);
    render(&ctx, &mut app, size, vec![key(Key::F10, true)]);
    let output = render(&ctx, &mut app, size, vec![key(Key::F10, false)]);
    click(&ctx, &mut app, size, text_rect(&output, "Configuration").center());

    let output = render(&ctx, &mut app, size, vec![]);
    let field_pos = |output: &FullOutput, label| text_rect(output, label).left_bottom() + Vec2::new(15.0, 13.0);
    click(&ctx, &mut app, size, field_pos(&output, "Logo (ruta a un PNG local)"));
    render(&ctx, &mut app, size, vec![Event::Text("logo.png".into())]);
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, field_pos(&output, "Archivo de texto UTF-8 (opcional)"));
    render(&ctx, &mut app, size, vec![Event::Text("marca.txt".into())]);
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Cargar texto").center());
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Guardar General").center());
    assert_eq!(app.settings.brand_text, "Mercado de prueba", "{:?}", app.dialog_error);
    assert!(app.brand_logo.is_some());
    assert_eq!(setting(&app.conn, "brand_logo").unwrap().as_deref(), Some("logo.png"));
    app.user = None;
    let output = render(&ctx, &mut app, size, vec![]);
    assert!(text_rect(&output, "Mercado de prueba").is_positive());
    app.login("0").unwrap();
    render(&ctx, &mut app, size, vec![key(Key::F10, true)]);
    let output = render(&ctx, &mut app, size, vec![key(Key::F10, false)]);
    click(&ctx, &mut app, size, text_rect(&output, "Configuration").center());
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Peripherals").center());
    let output = render(&ctx, &mut app, size, vec![]);
    click(&ctx, &mut app, size, text_rect(&output, "Escanea un código aquí").center());
    render(&ctx, &mut app, size, vec![Event::Text("123456".into()), key(Key::Enter, true)]);
    let output = render(&ctx, &mut app, size, vec![key(Key::Enter, false)]);
    assert!(text_rect(&output, "123456 · Café").is_positive());
    assert!(app.draft.is_empty() && pos_store::last_sale(&app.conn).unwrap().is_none());
    click(&ctx, &mut app, size, text_rect(&output, "Database Setup").center());
    let output = render(&ctx, &mut app, size, vec![]);
    assert!(text_rect(&output, "SQLite · un solo terminal").is_positive());
    assert!(text_rect(&output, "Verificar base activa").is_positive());
    assert!(open_prepared_database(&app.db).is_ok());
    assert!(open_prepared_database(&dir.path().join("marca.txt")).is_err());
    assert!(open_prepared_database(&dir.path().join("no-existe.db")).is_err());
    assert!(!dir.path().join("no-existe.db").exists());
}
