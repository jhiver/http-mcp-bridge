/**
 * Form Handler - Progressive enhancement for form submissions
 * Provides loading states, button disabling, and AJAX submission with fallback
 */

(function() {
    'use strict';

    // Initialize form handlers when DOM is ready
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', initFormHandlers);
    } else {
        initFormHandlers();
    }

    function initFormHandlers() {
        // Find all forms with data-enhance attribute or all forms if no specific marking
        const forms = document.querySelectorAll('form[data-enhance], form:not([data-no-enhance])');

        forms.forEach(form => {
            // Skip forms explicitly marked to not enhance
            if (form.hasAttribute('data-no-enhance')) return;

            enhanceForm(form);
        });
    }

    function enhanceForm(form) {
        const submitButton = form.querySelector('button[type="submit"], input[type="submit"]');
        if (!submitButton) return;

        // Store original button text
        const originalButtonText = submitButton.textContent || submitButton.value;

        // Create or find message container
        let messageContainer = form.querySelector('.form-message');
        if (!messageContainer) {
            messageContainer = document.createElement('div');
            messageContainer.className = 'form-message';
            messageContainer.style.display = 'none';
            form.insertBefore(messageContainer, form.firstChild);
        }

        // Handle form submission
        form.addEventListener('submit', async function(e) {
            // Check if form has data-ajax="true" for AJAX submission
            const useAjax = form.dataset.ajax === 'true';

            if (!useAjax) {
                // For traditional form submission, just disable the button
                disableSubmitButton(submitButton, originalButtonText);
                return; // Let the form submit normally
            }

            // Prevent default submission for AJAX forms
            e.preventDefault();

            // Clear previous messages
            clearMessage(messageContainer);

            // Disable submit button and show loading state
            disableSubmitButton(submitButton, originalButtonText);

            try {
                // Gather form data
                const formData = new FormData(form);
                const isMultipart = form.enctype === 'multipart/form-data';

                // Extract CSRF token from form if present
                const csrfToken = formData.get('csrf_token');

                // Prepare request options
                const options = {
                    method: form.method || 'POST',
                    headers: {
                        'X-Requested-With': 'XMLHttpRequest', // Mark as AJAX request
                    }
                };

                // Add CSRF token to headers if present
                if (csrfToken) {
                    options.headers['X-CSRF-Token'] = csrfToken;
                }

                // Set body based on form encoding
                if (isMultipart) {
                    options.body = formData;
                } else {
                    // Convert FormData to URLSearchParams for regular forms
                    const params = new URLSearchParams();
                    for (const [key, value] of formData) {
                        params.append(key, value);
                    }
                    options.body = params;
                    options.headers['Content-Type'] = 'application/x-www-form-urlencoded';
                }

                // Send the request
                const response = await fetch(form.action || window.location.href, options);
                const contentType = response.headers.get('content-type') || '';

                if (contentType.includes('application/json')) {
                    // Handle JSON response
                    const data = await response.json();

                    if (response.ok) {
                        showMessage(messageContainer, data.message || 'Success!', 'success');

                        // Clear form if specified
                        if (data.clearForm !== false) {
                            form.reset();
                        }

                        // Redirect if URL provided
                        if (data.redirect) {
                            setTimeout(() => {
                                window.location.href = data.redirect;
                            }, 1000);
                        }
                    } else {
                        showMessage(messageContainer, data.error || 'An error occurred', 'error');
                    }
                } else {
                    // Handle non-JSON response (fallback to page reload)
                    if (response.ok) {
                        // For successful non-JSON responses, check for redirect
                        const redirectUrl = response.headers.get('X-Redirect');
                        if (redirectUrl) {
                            window.location.href = redirectUrl;
                        } else {
                            // Fallback: reload the page to show server-rendered success message
                            window.location.reload();
                        }
                    } else {
                        showMessage(messageContainer, 'An error occurred. Please try again.', 'error');
                    }
                }
            } catch (error) {
                console.error('Form submission error:', error);
                showMessage(messageContainer, 'Network error. Please check your connection.', 'error');
            } finally {
                // Re-enable submit button
                enableSubmitButton(submitButton, originalButtonText);
            }
        });
    }

    function disableSubmitButton(button, originalText) {
        button.disabled = true;
        button.classList.add('loading');

        // Add loading text
        if (button.tagName === 'BUTTON') {
            button.innerHTML = '<span class="spinner"></span> Sending...';
        } else {
            button.value = 'Sending...';
        }
    }

    function enableSubmitButton(button, originalText) {
        button.disabled = false;
        button.classList.remove('loading');

        // Restore original text
        if (button.tagName === 'BUTTON') {
            button.textContent = originalText;
        } else {
            button.value = originalText;
        }
    }

    function showMessage(container, message, type) {
        container.className = `form-message alert alert-${type}`;
        container.textContent = message;
        container.style.display = 'block';

        // Auto-hide success messages after 5 seconds
        if (type === 'success') {
            setTimeout(() => {
                container.style.display = 'none';
            }, 5000);
        }
    }

    function clearMessage(container) {
        container.style.display = 'none';
        container.textContent = '';
        container.className = 'form-message';
    }

    // Expose functions for manual initialization if needed
    window.FormHandler = {
        init: initFormHandlers,
        enhance: enhanceForm
    };
})();