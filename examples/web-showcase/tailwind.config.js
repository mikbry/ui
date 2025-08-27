/** @type {import('tailwindcss').Config} */
module.exports = {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx,rs}",
    "../../../crates/mkui-web/src/**/*.rs",
    "./pkg/**/*.js",
  ],
  safelist: [
    // Layout & Flexbox
    'flex', 'flex-1', 'flex-col', 'flex-wrap', 'items-center', 'items-start', 'justify-center', 'justify-between', 'justify-start',
    
    // Spacing - gaps
    'gap-1', 'gap-2', 'gap-4', 'gap-6', 'gap-8',
    
    // Spacing - margins and padding
    'p-4', 'p-6', 'px-3', 'px-4', 'px-6', 'py-1', 'py-2', 'py-4', 
    'm-1', 'mr-3', 'mt-2', 'mt-4', 'mb-6',
    
    // Layout
    'container', 'mx-auto', 'min-h-screen', 'h-8', 'h-9', 'h-16',
    
    // Grid
    'grid', 'grid-cols-3', 'grid-cols-4', 'grid-cols-6',
    
    // Responsive grid
    'sm:grid-cols-4', 'sm:block', 'md:grid-cols-6',
    
    // Text & Typography
    'text-xs', 'text-sm', 'text-xl', 'text-2xl', 'font-medium', 'font-semibold', 'leading-none',
    
    // Colors & Themes
    'bg-background', 'bg-surface', 'bg-primary', 'bg-accent', 'bg-card', 
    'text-foreground', 'text-primary', 'text-primary-foreground', 'text-muted-foreground',
    'text-accent-foreground', 'text-card-foreground',
    'border', 'border-b', 'border-input',
    
    // Button styles
    'inline-flex', 'whitespace-nowrap', 'rounded-md', 'ring-offset-background',
    'transition-colors', 'focus-visible:outline-none', 'focus-visible:ring-2', 'focus-visible:ring-ring', 
    'focus-visible:ring-offset-2', 'disabled:pointer-events-none', 'disabled:opacity-50',
    'hover:bg-primary/90', 'hover:bg-accent', 'hover:text-accent-foreground',
    
    // Spacing utilities
    'space-x-4', 'space-y-2',
    
    // Display
    'hidden', 'block',
    
    // Shadows
    'shadow-sm',
    
    // Border radius
    'rounded-lg',
  ],
  theme: {
    extend: {},
  },
  plugins: [],
}