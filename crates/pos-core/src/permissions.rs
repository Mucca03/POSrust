/// Acciones que el POS sabe autorizar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Capability {
    Sell,
    RemoveLine,
    LineDiscount,
    TicketDiscount,
    ChangePrice,
    Reprint,
    OpenDrawer,
    CashMovement,
    CloseSession,
    Backoffice,
    Reports,
    Settings,
}

impl Capability {
    pub const ALL: [Self; 12] = [
        Self::Sell,
        Self::RemoveLine,
        Self::LineDiscount,
        Self::TicketDiscount,
        Self::ChangePrice,
        Self::Reprint,
        Self::OpenDrawer,
        Self::CashMovement,
        Self::CloseSession,
        Self::Backoffice,
        Self::Reports,
        Self::Settings,
    ];
}

/// Roles admitidos al migrar bases existentes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RoleKind {
    Admin,
    Manager,
    Employee,
    Guest,
}

impl RoleKind {
    pub fn from_legacy_id(id: &str) -> Option<Self> {
        match id {
            "0" => Some(Self::Admin),
            "1" => Some(Self::Manager),
            "2" => Some(Self::Employee),
            "3" => Some(Self::Guest),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Admin => "Administrador",
            Self::Manager => "Gerente",
            Self::Employee => "Empleado",
            Self::Guest => "Invitado",
        }
    }

    pub const fn permissions(self) -> Permissions {
        match self {
            Self::Admin => Permissions {
                sell: true,
                remove_line: true,
                line_discount: true,
                ticket_discount: true,
                change_price: true,
                reprint: true,
                open_drawer: true,
                cash_movement: true,
                close_session: true,
                backoffice: true,
                reports: true,
                settings: true,
            },
            Self::Manager => Permissions {
                sell: true,
                remove_line: true,
                line_discount: true,
                ticket_discount: true,
                change_price: true,
                reprint: true,
                open_drawer: true,
                cash_movement: true,
                close_session: true,
                backoffice: true,
                reports: true,
                settings: false,
            },
            Self::Employee => Permissions {
                sell: true,
                remove_line: true,
                line_discount: false,
                ticket_discount: false,
                change_price: false,
                reprint: true,
                open_drawer: true,
                cash_movement: false,
                close_session: false,
                backoffice: false,
                reports: false,
                settings: false,
            },
            Self::Guest => Permissions::NONE,
        }
    }

    pub const fn can(self, capability: Capability) -> bool {
        self.permissions().can(capability)
    }
}

/// Permisos efectivos de un usuario.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permissions {
    pub sell: bool,
    pub remove_line: bool,
    pub line_discount: bool,
    pub ticket_discount: bool,
    pub change_price: bool,
    pub reprint: bool,
    pub open_drawer: bool,
    pub cash_movement: bool,
    pub close_session: bool,
    pub backoffice: bool,
    pub reports: bool,
    pub settings: bool,
}

impl Permissions {
    pub const NONE: Self = Self {
        sell: false,
        remove_line: false,
        line_discount: false,
        ticket_discount: false,
        change_price: false,
        reprint: false,
        open_drawer: false,
        cash_movement: false,
        close_session: false,
        backoffice: false,
        reports: false,
        settings: false,
    };

    pub const fn can(self, capability: Capability) -> bool {
        match capability {
            Capability::Sell => self.sell,
            Capability::RemoveLine => self.remove_line,
            Capability::LineDiscount => self.line_discount,
            Capability::TicketDiscount => self.ticket_discount,
            Capability::ChangePrice => self.change_price,
            Capability::Reprint => self.reprint,
            Capability::OpenDrawer => self.open_drawer,
            Capability::CashMovement => self.cash_movement,
            Capability::CloseSession => self.close_session,
            Capability::Backoffice => self.backoffice,
            Capability::Reports => self.reports,
            Capability::Settings => self.settings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_puede_todo() {
        for capability in Capability::ALL {
            assert!(RoleKind::Admin.can(capability), "{capability:?}");
        }
    }

    #[test]
    fn gerente_no_configura() {
        assert!(RoleKind::Manager.can(Capability::CloseSession));
        assert!(RoleKind::Manager.can(Capability::Reports));
        assert!(!RoleKind::Manager.can(Capability::Settings));
    }

    #[test]
    fn empleado_solo_opera_la_caja() {
        assert!(RoleKind::Employee.can(Capability::Sell));
        assert!(RoleKind::Employee.can(Capability::RemoveLine));
        assert!(RoleKind::Employee.can(Capability::Reprint));
        assert!(RoleKind::Employee.can(Capability::OpenDrawer));
        assert!(!RoleKind::Employee.can(Capability::LineDiscount));
        assert!(!RoleKind::Employee.can(Capability::TicketDiscount));
        assert!(!RoleKind::Employee.can(Capability::CashMovement));
        assert!(!RoleKind::Employee.can(Capability::CloseSession));
        assert!(!RoleKind::Employee.can(Capability::Reports));
    }

    #[test]
    fn invitado_no_puede_nada() {
        for capability in Capability::ALL {
            assert!(!RoleKind::Guest.can(capability), "{capability:?}");
        }
    }
}
