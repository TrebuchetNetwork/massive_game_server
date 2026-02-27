/**
 * VOID STRIKE - Main JavaScript
 * Handles animations, interactions, and scroll behaviors
 */

// ============================================
// DOM Ready
// ============================================
document.addEventListener('DOMContentLoaded', function() {
    initNavigation();
    initScrollAnimations();
    initMobileMenu();
    initSmoothScroll();
    initParallaxEffect();
    initCounterAnimations();
    initGalleryLightbox();
});

// ============================================
// Navigation
// ============================================
function initNavigation() {
    const navbar = document.getElementById('navbar');
    let lastScroll = 0;
    
    window.addEventListener('scroll', () => {
        const currentScroll = window.pageYOffset;
        
        // Add/remove scrolled class for background
        if (currentScroll > 50) {
            navbar.classList.add('scrolled');
        } else {
            navbar.classList.remove('scrolled');
        }
        
        // Hide/show navbar on scroll direction (optional)
        if (currentScroll > lastScroll && currentScroll > 200) {
            navbar.style.transform = 'translateY(-100%)';
        } else {
            navbar.style.transform = 'translateY(0)';
        }
        
        lastScroll = currentScroll;
    });
    
    // Smooth transition for navbar
    navbar.style.transition = 'transform 0.3s ease, background 0.3s ease';
}

// ============================================
// Scroll Animations (Intersection Observer)
// ============================================
function initScrollAnimations() {
    const observerOptions = {
        root: null,
        rootMargin: '0px',
        threshold: 0.1
    };
    
    const observer = new IntersectionObserver((entries) => {
        entries.forEach(entry => {
            if (entry.isIntersecting) {
                entry.target.classList.add('revealed');
                
                // Add stagger animation for child elements
                const children = entry.target.querySelectorAll('.stagger-child');
                children.forEach((child, index) => {
                    setTimeout(() => {
                        child.classList.add('revealed');
                    }, index * 100);
                });
                
                // Unobserve after animation
                observer.unobserve(entry.target);
            }
        });
    }, observerOptions);
    
    // Observe elements with scroll-reveal classes
    const revealElements = document.querySelectorAll(
        '.feature-card, .mode-card, .ship-card, .reward-card, ' +
        '.section-header, .stat-card, .gallery-item, .rank-tier, ' +
        '.career-stat'
    );
    
    revealElements.forEach((el, index) => {
        el.classList.add('scroll-reveal');
        // Add stagger delay based on position
        const delay = (index % 6) * 0.1;
        el.style.transitionDelay = `${delay}s`;
        observer.observe(el);
    });
}

// ============================================
// Mobile Menu
// ============================================
function initMobileMenu() {
    const menuBtn = document.getElementById('mobile-menu-btn');
    const mobileMenu = document.getElementById('mobile-menu');
    
    if (!menuBtn || !mobileMenu) return;
    
    menuBtn.addEventListener('click', () => {
        mobileMenu.classList.toggle('active');
        mobileMenu.classList.toggle('hidden');
        
        // Toggle icon
        const icon = menuBtn.querySelector('i');
        if (mobileMenu.classList.contains('active')) {
            icon.classList.remove('fa-bars');
            icon.classList.add('fa-times');
        } else {
            icon.classList.remove('fa-times');
            icon.classList.add('fa-bars');
        }
    });
    
    // Close menu when clicking a link
    const mobileLinks = mobileMenu.querySelectorAll('a');
    mobileLinks.forEach(link => {
        link.addEventListener('click', () => {
            mobileMenu.classList.remove('active');
            mobileMenu.classList.add('hidden');
            const icon = menuBtn.querySelector('i');
            icon.classList.remove('fa-times');
            icon.classList.add('fa-bars');
        });
    });
}

// ============================================
// Smooth Scroll for Anchor Links
// ============================================
function initSmoothScroll() {
    document.querySelectorAll('a[href^="#"]').forEach(anchor => {
        anchor.addEventListener('click', function(e) {
            e.preventDefault();
            const targetId = this.getAttribute('href');
            
            if (targetId === '#') return;
            
            const targetElement = document.querySelector(targetId);
            if (targetElement) {
                const navHeight = document.getElementById('navbar').offsetHeight;
                const targetPosition = targetElement.offsetTop - navHeight;
                
                window.scrollTo({
                    top: targetPosition,
                    behavior: 'smooth'
                });
            }
        });
    });
}

// ============================================
// Parallax Effect for Hero Section
// ============================================
function initParallaxEffect() {
    const heroGlows = document.querySelectorAll('.hero-glow');
    
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
        return;
    }
    
    let ticking = false;
    
    window.addEventListener('scroll', () => {
        if (!ticking) {
            window.requestAnimationFrame(() => {
                const scrolled = window.pageYOffset;
                const rate = scrolled * 0.3;
                
                heroGlows.forEach((glow, index) => {
                    const direction = index % 2 === 0 ? 1 : -1;
                    glow.style.transform = `translateY(${rate * direction}px)`;
                });
                
                ticking = false;
            });
            
            ticking = true;
        }
    });
}

// ============================================
// Counter Animations
// ============================================
function initCounterAnimations() {
    const counters = document.querySelectorAll('.stat-card .text-3xl, .career-stat .text-4xl');
    
    const counterObserver = new IntersectionObserver((entries) => {
        entries.forEach(entry => {
            if (entry.isIntersecting) {
                const counter = entry.target;
                const text = counter.textContent;
                const numericValue = parseFloat(text.replace(/[^0-9.]/g, ''));
                const suffix = text.replace(/[0-9.]/g, '');
                
                if (!isNaN(numericValue)) {
                    animateCounter(counter, 0, numericValue, 1500, suffix);
                }
                
                counterObserver.unobserve(counter);
            }
        });
    }, { threshold: 0.5 });
    
    counters.forEach(counter => counterObserver.observe(counter));
}

function animateCounter(element, start, end, duration, suffix = '') {
    const startTime = performance.now();
    const isDecimal = end % 1 !== 0;
    
    function update(currentTime) {
        const elapsed = currentTime - startTime;
        const progress = Math.min(elapsed / duration, 1);
        
        // Easing function (ease-out)
        const easeOut = 1 - Math.pow(1 - progress, 3);
        const current = start + (end - start) * easeOut;
        
        if (isDecimal) {
            element.textContent = current.toFixed(1) + suffix;
        } else {
            element.textContent = Math.floor(current) + suffix;
        }
        
        if (progress < 1) {
            requestAnimationFrame(update);
        }
    }
    
    requestAnimationFrame(update);
}

// ============================================
// Gallery Lightbox
// ============================================
function initGalleryLightbox() {
    const galleryItems = document.querySelectorAll('.gallery-item');
    
    galleryItems.forEach(item => {
        item.addEventListener('click', () => {
            // Create lightbox
            const lightbox = document.createElement('div');
            lightbox.className = 'fixed inset-0 z-50 flex items-center justify-center bg-black/90 backdrop-blur-sm';
            lightbox.innerHTML = `
                <div class="relative max-w-4xl max-h-[90vh] p-4">
                    <button class="absolute -top-12 right-0 text-white hover:text-void-cyan transition-colors">
                        <i class="fas fa-times text-2xl"></i>
                    </button>
                    <div class="bg-void-card rounded-xl p-8 text-center">
                        <i class="fas fa-image text-6xl text-white/20 mb-4"></i>
                        <p class="text-gray-400">Screenshot coming soon</p>
                    </div>
                </div>
            `;
            
            document.body.appendChild(lightbox);
            document.body.style.overflow = 'hidden';
            
            // Close on click
            lightbox.addEventListener('click', (e) => {
                if (e.target === lightbox || e.target.closest('button')) {
                    lightbox.remove();
                    document.body.style.overflow = '';
                }
            });
            
            // Close on escape key
            const closeOnEscape = (e) => {
                if (e.key === 'Escape') {
                    lightbox.remove();
                    document.body.style.overflow = '';
                    document.removeEventListener('keydown', closeOnEscape);
                }
            };
            document.addEventListener('keydown', closeOnEscape);
        });
    });
}

// ============================================
// Utility Functions
// ============================================

/**
 * Debounce function
 */
function debounce(func, wait) {
    let timeout;
    return function executedFunction(...args) {
        const later = () => {
            clearTimeout(timeout);
            func(...args);
        };
        clearTimeout(timeout);
        timeout = setTimeout(later, wait);
    };
}

/**
 * Throttle function
 */
function throttle(func, limit) {
    let inThrottle;
    return function(...args) {
        if (!inThrottle) {
            func.apply(this, args);
            inThrottle = true;
            setTimeout(() => inThrottle = false, limit);
        }
    };
}

// ============================================
// Loading Screen (Optional)
// ============================================
function showLoadingScreen() {
    const loader = document.createElement('div');
    loader.id = 'loading-screen';
    loader.className = 'fixed inset-0 z-[9999] bg-void-dark flex items-center justify-center';
    loader.innerHTML = `
        <div class="text-center">
            <div class="w-16 h-16 mx-auto mb-4 rounded-full border-4 border-void-cyan/20 border-t-void-cyan animate-spin"></div>
            <p class="font-orbitron text-void-cyan tracking-wider">LOADING...</p>
        </div>
    `;
    
    document.body.appendChild(loader);
    
    // Hide after page loads
    window.addEventListener('load', () => {
        setTimeout(() => {
            loader.style.opacity = '0';
            loader.style.transition = 'opacity 0.5s ease';
            setTimeout(() => loader.remove(), 500);
        }, 500);
    });
}

// ============================================
// Coming Soon Popup
// ============================================
document.querySelectorAll('a[href="#"]').forEach(link => {
    link.addEventListener('click', (e) => {
        // Only show popup for actual placeholder links
        if (link.getAttribute('href') === '#' && !link.closest('#mobile-menu') && !link.closest('nav')) {
            e.preventDefault();
            showComingSoonPopup();
        }
    });
});

function showComingSoonPopup() {
    // Remove existing popup
    const existingPopup = document.getElementById('coming-soon-popup');
    if (existingPopup) existingPopup.remove();
    
    const popup = document.createElement('div');
    popup.id = 'coming-soon-popup';
    popup.className = 'fixed top-24 left-1/2 -translate-x-1/2 z-[100] px-6 py-3 rounded-lg bg-void-card border border-void-cyan/50 shadow-lg shadow-void-cyan/20';
    popup.innerHTML = `
        <div class="flex items-center gap-3">
            <i class="fas fa-rocket text-void-cyan"></i>
            <span class="font-orbitron text-sm">Coming Soon!</span>
        </div>
    `;
    
    document.body.appendChild(popup);
    
    // Animate in
    popup.style.opacity = '0';
    popup.style.transform = 'translateX(-50%) translateY(-20px)';
    setTimeout(() => {
        popup.style.transition = 'all 0.3s ease';
        popup.style.opacity = '1';
        popup.style.transform = 'translateX(-50%) translateY(0)';
    }, 10);
    
    // Remove after delay
    setTimeout(() => {
        popup.style.opacity = '0';
        popup.style.transform = 'translateX(-50%) translateY(-20px)';
        setTimeout(() => popup.remove(), 300);
    }, 2000);
}

// ============================================
// Keyboard Navigation
// ============================================
document.addEventListener('keydown', (e) => {
    // Press '?' to show keyboard shortcuts
    if (e.key === '?' && !e.target.matches('input, textarea')) {
        e.preventDefault();
        showKeyboardShortcuts();
    }
});

function showKeyboardShortcuts() {
    const modal = document.createElement('div');
    modal.className = 'fixed inset-0 z-[200] flex items-center justify-center bg-black/80 backdrop-blur-sm';
    modal.innerHTML = `
        <div class="bg-void-card rounded-2xl p-8 max-w-md w-full mx-4 border border-white/10">
            <h3 class="font-orbitron font-bold text-xl mb-6 text-center">Keyboard Shortcuts</h3>
            <div class="space-y-3">
                <div class="flex justify-between">
                    <span class="text-gray-400">?</span>
                    <span class="text-white">Show this help</span>
                </div>
                <div class="flex justify-between">
                    <span class="text-gray-400">Esc</span>
                    <span class="text-white">Close modals</span>
                </div>
                <div class="flex justify-between">
                    <span class="text-gray-400">Home</span>
                    <span class="text-white">Go to top</span>
                </div>
            </div>
            <button class="mt-6 w-full py-3 rounded-lg bg-void-cyan/20 text-void-cyan font-orbitron hover:bg-void-cyan/30 transition-colors">
                Close
            </button>
        </div>
    `;
    
    document.body.appendChild(modal);
    
    modal.addEventListener('click', (e) => {
        if (e.target === modal || e.target.tagName === 'BUTTON') {
            modal.remove();
        }
    });
}

// ============================================
// Performance: Lazy Load Images (if needed)
// ============================================
function initLazyLoading() {
    const lazyImages = document.querySelectorAll('img[data-src]');
    
    const imageObserver = new IntersectionObserver((entries) => {
        entries.forEach(entry => {
            if (entry.isIntersecting) {
                const img = entry.target;
                img.src = img.dataset.src;
                img.removeAttribute('data-src');
                imageObserver.unobserve(img);
            }
        });
    });
    
    lazyImages.forEach(img => imageObserver.observe(img));
}

// ============================================
// Console Easter Egg
// ============================================
console.log('%c VOID STRIKE ', 'background: linear-gradient(135deg, #00D4FF, #9D4EDD); color: #0A0A0F; font-size: 24px; font-weight: bold; padding: 10px 20px; border-radius: 8px;');
console.log('%c Welcome, Pilot! ', 'color: #00D4FF; font-size: 14px;');
console.log('%c Ready to enter the void? ', 'color: #FF006E; font-size: 12px;');
