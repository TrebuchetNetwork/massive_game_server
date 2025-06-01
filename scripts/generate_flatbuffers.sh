#!/bin/bash

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${GREEN}FlatBuffers JavaScript/TypeScript Generator${NC}"
echo "==========================================="

# Check if flatc is installed
if ! command -v flatc &> /dev/null; then
    echo -e "${RED}Error: flatc is not installed!${NC}"
    echo ""
    echo "Installation instructions:"
    echo "  Mac:     brew install flatbuffers"
    echo "  Ubuntu:  sudo apt-get install flatbuffers-compiler"
    echo "  Other:   Visit https://google.github.io/flatbuffers/"
    exit 1
fi

# Print flatc version
echo -e "${YELLOW}Using flatc version:${NC}"
flatc --version
echo ""

# Set variables
SCHEMA_FILE="../server/schemas/game.fbs"
OUTPUT_DIR="../static_client/generated_js"
SERVER_OUTPUT_DIR="../target/flatbuffers"

# Parse command line arguments
SKIP_TSC=false
INSTALL_TSC=false
while [[ $# -gt 0 ]]; do
    case $1 in
        --skip-tsc)
            SKIP_TSC=true
            shift
            ;;
        --install-tsc)
            INSTALL_TSC=true
            shift
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            echo "Usage: $0 [--skip-tsc] [--install-tsc]"
            echo "  --skip-tsc     Skip TypeScript compilation"
            echo "  --install-tsc  Install TypeScript compiler if not found"
            exit 1
            ;;
    esac
done

# Check if schema file exists
if [ ! -f "$SCHEMA_FILE" ]; then
    echo -e "${RED}Error: Schema file '$SCHEMA_FILE' not found!${NC}"
    echo "Please ensure the FlatBuffers schema file exists in the correct location."
    exit 1
fi

# Create output directories if they don't exist
echo "Creating output directories..."
mkdir -p "$OUTPUT_DIR"
mkdir -p "$SERVER_OUTPUT_DIR"

# Clean up old generated files
echo -e "${YELLOW}Cleaning up old generated files...${NC}"
rm -f "$OUTPUT_DIR"/*.ts "$OUTPUT_DIR"/*.js "$OUTPUT_DIR"/*.d.ts

# Generate TypeScript files for the client
echo -e "${YELLOW}Generating TypeScript files...${NC}"
if flatc --ts -o "$OUTPUT_DIR" "$SCHEMA_FILE"; then
    echo -e "${GREEN}✓ TypeScript files generated successfully in $OUTPUT_DIR${NC}"
    
    # TypeScript compilation
    if [ "$SKIP_TSC" = false ]; then
        # Check if TypeScript compiler is available
        if ! command -v tsc &> /dev/null; then
            echo -e "${YELLOW}⚠ TypeScript compiler not found.${NC}"
            
            if [ "$INSTALL_TSC" = true ]; then
                echo -e "${BLUE}Installing TypeScript...${NC}"
                if command -v npm &> /dev/null; then
                    npm install -g typescript
                    if command -v tsc &> /dev/null; then
                        echo -e "${GREEN}✓ TypeScript installed successfully${NC}"
                    else
                        echo -e "${RED}✗ Failed to install TypeScript${NC}"
                        exit 1
                    fi
                else
                    echo -e "${RED}npm not found. Cannot install TypeScript.${NC}"
                    echo "Please install Node.js/npm first or run with --skip-tsc"
                    exit 1
                fi
            else
                echo ""
                echo "To compile TypeScript files, you can:"
                echo "  1. Install TypeScript: npm install -g typescript"
                echo "  2. Run this script with --install-tsc flag"
                echo "  3. Run with --skip-tsc to skip compilation"
                echo ""
                echo "Note: Modern browsers can use .ts files directly with type=\"module\""
                exit 1
            fi
        fi
        
        # Now compile TypeScript
        echo -e "${YELLOW}Compiling TypeScript to JavaScript...${NC}"
        
        # Create a tsconfig.json for compilation
        TSCONFIG_FILE="$OUTPUT_DIR/tsconfig.json"
        cat > "$TSCONFIG_FILE" << EOF
{
  "compilerOptions": {
    "target": "ES2015",
    "module": "ES2015",
    "lib": ["ES2015", "DOM"],
    "declaration": true,
    "declarationMap": true,
    "sourceMap": true,
    "outDir": "./",
    "rootDir": "./",
    "strict": false,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "forceConsistentCasingInFileNames": true,
    "moduleResolution": "node",
    "allowJs": true,
    "resolveJsonModule": true
  },
  "include": ["./*.ts"],
  "exclude": ["node_modules", "**/*.spec.ts"]
}
EOF
        
        # Save current directory
        CURRENT_DIR=$(pwd)
        
        # Change to output directory and compile
        cd "$OUTPUT_DIR"
        
        echo -e "${BLUE}Running: tsc${NC}"
        if tsc; then
            echo -e "${GREEN}✓ JavaScript files compiled successfully${NC}"
            
            # Count generated files
            TS_COUNT=$(ls -1 *.ts 2>/dev/null | wc -l)
            JS_COUNT=$(ls -1 *.js 2>/dev/null | wc -l)
            DTS_COUNT=$(ls -1 *.d.ts 2>/dev/null | wc -l)
            
            echo -e "${GREEN}  Generated: ${TS_COUNT} .ts files, ${JS_COUNT} .js files, ${DTS_COUNT} .d.ts files${NC}"
        else
            echo -e "${RED}✗ TypeScript compilation failed${NC}"
            echo "Error details above. The .ts files are still available."
        fi
        
        # Clean up tsconfig.json
        rm -f tsconfig.json
        
        # Return to original directory
        cd "$CURRENT_DIR"
    else
        echo -e "${YELLOW}Skipping TypeScript compilation (--skip-tsc flag used)${NC}"
    fi
else
    echo -e "${RED}✗ Failed to generate TypeScript files${NC}"
    exit 1
fi

# Also generate for server (Rust) if needed
echo ""
echo -e "${YELLOW}Generating Rust files for server...${NC}"
if flatc --rust -o "$SERVER_OUTPUT_DIR" "$SCHEMA_FILE"; then
    echo -e "${GREEN}✓ Server files generated successfully in $SERVER_OUTPUT_DIR${NC}"
else
    echo -e "${RED}✗ Failed to generate server files${NC}"
    exit 1
fi

# List generated files
echo ""
echo -e "${GREEN}Generated files summary:${NC}"
echo ""
echo -e "${BLUE}Client (TypeScript/JavaScript):${NC}"
if ls "$OUTPUT_DIR"/*.ts &>/dev/null; then
    for file in "$OUTPUT_DIR"/*.ts; do
        echo "  📄 $(basename "$file")"
    done
fi

if ls "$OUTPUT_DIR"/*.js &>/dev/null; then
    echo ""
    echo -e "${BLUE}Compiled JavaScript:${NC}"
    for file in "$OUTPUT_DIR"/*.js; do
        echo "  📄 $(basename "$file")"
    done
fi

echo ""
echo -e "${BLUE}Server (Rust):${NC}"
if ls "$SERVER_OUTPUT_DIR"/*.rs &>/dev/null; then
    for file in "$SERVER_OUTPUT_DIR"/*.rs; do
        echo "  📄 $(basename "$file")"
    done
fi

# Usage instructions
echo ""
echo -e "${GREEN}✅ FlatBuffers generation complete!${NC}"
echo ""
echo -e "${YELLOW}Usage in your client:${NC}"
echo ""
echo "Option 1 - Using compiled JavaScript (ES6 modules):"
echo '  <script type="module">'
echo '    import { Game } from "./generated_js/game_generated.js";'
echo '    // Your code here'
echo '  </script>'
echo ""
echo "Option 2 - Using TypeScript directly (with bundler):"
echo '  import { Game } from "./generated_js/game_generated";'
echo ""
echo "Option 3 - Using CommonJS (Node.js):"
echo '  const { Game } = require("./generated_js/game_generated");'
echo ""
echo -e "${YELLOW}Script options:${NC}"
echo "  $0 --skip-tsc      # Skip TypeScript compilation"
echo "  $0 --install-tsc   # Auto-install TypeScript if missing"