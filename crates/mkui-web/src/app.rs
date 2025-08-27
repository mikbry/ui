use wasm_bindgen::prelude::*;
use crate::renderer::WebRenderer;
use mkui_core::theme::{ThemeMode, ColorTheme};

pub struct WebApp {
    renderer: WebRenderer,
    theme_mode: ThemeMode,
    color_theme: ColorTheme,
}

impl WebApp {
    pub fn new(root_id: &str) -> Result<Self, JsValue> {
        let mut app = Self {
            renderer: WebRenderer::new(root_id)?,
            theme_mode: ThemeMode::System,
            color_theme: ColorTheme::Default,
        };
        
        // Load saved theme preferences
        app.load_theme_preferences();
        
        // Apply initial theme
        app.apply_theme()?;
        
        Ok(app)
    }
    
    pub fn renderer(&self) -> &WebRenderer {
        &self.renderer
    }
    
    pub fn mount(&self) -> Result<(), JsValue> {
        // Apply theme on mount
        self.apply_theme()?;
        Ok(())
    }
    
    pub fn set_theme_mode(&mut self, mode: ThemeMode) -> Result<(), JsValue> {
        self.theme_mode = mode;
        self.save_theme_preferences();
        self.apply_theme()
    }
    
    pub fn set_color_theme(&mut self, theme: ColorTheme) -> Result<(), JsValue> {
        self.color_theme = theme;
        self.save_theme_preferences();
        self.apply_theme()
    }
    
    pub fn get_theme_mode(&self) -> ThemeMode {
        self.theme_mode
    }
    
    pub fn get_color_theme(&self) -> &ColorTheme {
        &self.color_theme
    }
    
    fn apply_theme(&self) -> Result<(), JsValue> {
        use crate::utils::document;
        let document = document();
        
        // Get the html element to apply theme classes
        let html = document.document_element()
            .ok_or_else(|| JsValue::from_str("No html element found"))?;
        
        let html_class_list = html.class_list();
        
        // Clear all color theme classes from html
        for theme in ColorTheme::all() {
            html_class_list.remove_1(theme.to_class())?;
        }
        
        // Apply color theme to html element
        html_class_list.add_1(self.color_theme.to_class())?;
        
        // Apply dark mode to body if it exists
        if let Some(body) = document.body() {
            let body_class_list = body.class_list();
            
            // Clear theme mode classes
            body_class_list.remove_1("dark")?;
            body_class_list.remove_1("light")?;
            
            // Apply theme mode
            match self.theme_mode {
                ThemeMode::Dark => {
                    body_class_list.add_1("dark")?;
                },
                ThemeMode::Light => {
                    // Light mode is default, no class needed
                },
                ThemeMode::System => {
                    // Check system preference
                    if self.is_system_dark_mode() {
                        body_class_list.add_1("dark")?;
                    }
                }
            }
        }
        
        Ok(())
    }
    
    fn is_system_dark_mode(&self) -> bool {
        use crate::utils::window;
        let window = window();
        
        // Check if matchMedia is available
        if let Ok(Some(media_query)) = window.match_media("(prefers-color-scheme: dark)") {
            return media_query.matches();
        }
        
        false
    }
    
    fn save_theme_preferences(&self) {
        use crate::utils::window;
        
        if let Ok(Some(storage)) = window().local_storage() {
            let theme_mode = match self.theme_mode {
                ThemeMode::Light => "light",
                ThemeMode::Dark => "dark",
                ThemeMode::System => "system",
            };
            
            let color_theme = match &self.color_theme {
                ColorTheme::Default => "default",
                ColorTheme::Blue => "blue",
                ColorTheme::Green => "green",
                ColorTheme::Amber => "amber",
                ColorTheme::Rose => "rose",
                ColorTheme::Purple => "purple",
                ColorTheme::Orange => "orange",
                ColorTheme::Teal => "teal",
                ColorTheme::Mono => "mono",
                ColorTheme::Scaled => "scaled",
                ColorTheme::Red => "red",
                ColorTheme::Yellow => "yellow",
                ColorTheme::Violet => "violet",
            };
            
            let _ = storage.set_item("mkui-theme-mode", theme_mode);
            let _ = storage.set_item("mkui-color-theme", color_theme);
        }
    }
    
    fn load_theme_preferences(&mut self) {
        use crate::utils::window;
        
        if let Ok(Some(storage)) = window().local_storage() {
            // Load theme mode
            if let Ok(Some(mode)) = storage.get_item("mkui-theme-mode") {
                self.theme_mode = match mode.as_str() {
                    "light" => ThemeMode::Light,
                    "dark" => ThemeMode::Dark,
                    "system" => ThemeMode::System,
                    _ => ThemeMode::System,
                };
            }
            
            // Load color theme
            if let Ok(Some(theme_str)) = storage.get_item("mkui-color-theme") {
                if let Some(color_theme) = ColorTheme::from_str(&theme_str) {
                    self.color_theme = color_theme;
                }
            }
        }
    }
}