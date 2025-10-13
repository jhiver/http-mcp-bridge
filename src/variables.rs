use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

// Hardcoded regex pattern - guaranteed to be valid at compile time
// Using unwrap here is safe because the pattern is a compile-time constant
#[allow(clippy::unwrap_used)]
static VARIABLE_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\{\{([a-zA-Z_][a-zA-Z0-9_:]*)\}\}").unwrap());

pub struct VariableEngine {
    pattern: &'static Regex,
}

impl VariableEngine {
    pub fn new() -> Self {
        Self {
            // Match {{variable}} or {{type:variable}} (type support for Phase 3)
            pattern: &VARIABLE_PATTERN,
        }
    }

    /// Extract variable names from a template string
    pub fn extract_variables(&self, text: &str) -> Vec<String> {
        self.pattern
            .captures_iter(text)
            .map(|cap| cap[1].to_string())
            .collect()
    }

    /// Substitute variables in a template with values from context
    pub fn substitute(&self, template: &str, context: &HashMap<String, String>) -> String {
        self.pattern
            .replace_all(template, |caps: &regex::Captures| {
                let var_spec = &caps[1];

                // For now, simple substitution (Phase 3 will add type casting)
                // Check if variable has a type prefix (e.g., "int:count")
                let var_name = if var_spec.contains(':') {
                    var_spec.split(':').nth(1).unwrap_or(var_spec)
                } else {
                    var_spec
                };

                // Get value from context or keep original placeholder
                context
                    .get(var_name)
                    .cloned()
                    .unwrap_or_else(|| format!("{{{{ {} }}}}", var_spec))
            })
            .to_string()
    }

    /// Check if a string contains any variables
    pub fn has_variables(&self, text: &str) -> bool {
        self.pattern.is_match(text)
    }

    /// Count the number of unique variables in a string
    pub fn count_unique_variables(&self, text: &str) -> usize {
        let vars = self.extract_variables(text);
        let mut unique = Vec::new();
        for var in vars {
            if !unique.contains(&var) {
                unique.push(var);
            }
        }
        unique.len()
    }
}

impl Default for VariableEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variable_extraction() {
        let engine = VariableEngine::new();
        let text = "URL: {{base_url}}/api/{{version}}/users";
        let vars = engine.extract_variables(text);
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0], "base_url");
        assert_eq!(vars[1], "version");
    }

    #[test]
    fn test_substitution() {
        let engine = VariableEngine::new();
        let template = "{{greeting}}, {{name}}!";
        let mut context = HashMap::new();
        context.insert("greeting".to_string(), "Hello".to_string());
        context.insert("name".to_string(), "World".to_string());

        let result = engine.substitute(template, &context);
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_partial_substitution() {
        let engine = VariableEngine::new();
        let template = "{{greeting}}, {{name}}! Today is {{day}}.";
        let mut context = HashMap::new();
        context.insert("greeting".to_string(), "Hello".to_string());
        context.insert("name".to_string(), "World".to_string());
        // Don't provide 'day' - it should remain as {{day}}

        let result = engine.substitute(template, &context);
        assert_eq!(result, "Hello, World! Today is {{ day }}.");
    }

    #[test]
    fn test_has_variables() {
        let engine = VariableEngine::new();
        assert!(engine.has_variables("This has {{a_variable}}"));
        assert!(engine.has_variables("{{start}} middle {{end}}"));
        assert!(!engine.has_variables("This has no variables"));
    }

    #[test]
    fn test_count_unique_variables() {
        let engine = VariableEngine::new();

        // No variables
        assert_eq!(engine.count_unique_variables("No variables here"), 0);

        // One unique variable
        assert_eq!(engine.count_unique_variables("{{var}}"), 1);

        // Multiple same variables
        assert_eq!(
            engine.count_unique_variables("{{var}} and {{var}} again"),
            1
        );

        // Multiple different variables
        assert_eq!(engine.count_unique_variables("{{a}} {{b}} {{c}}"), 3);

        // Mixed unique and duplicate
        assert_eq!(engine.count_unique_variables("{{a}} {{b}} {{a}} {{c}}"), 3);
    }

    #[test]
    fn test_typed_variables() {
        let engine = VariableEngine::new();

        // Extract typed variables (for future use)
        let text = "Count: {{int:count}}, Name: {{string:name}}";
        let vars = engine.extract_variables(text);
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0], "int:count");
        assert_eq!(vars[1], "string:name");

        // Substitution works with typed variables
        let mut context = HashMap::new();
        context.insert("count".to_string(), "42".to_string());
        context.insert("name".to_string(), "Test".to_string());

        let result = engine.substitute(text, &context);
        assert_eq!(result, "Count: 42, Name: Test");
    }
}
