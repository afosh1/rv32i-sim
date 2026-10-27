// How many Collatz steps does 27 take to reach 1? (Famously: 111.)
// The answer is returned from main, which crt0.S turns into exit(111).
//
// RV32I has no multiply instruction; GCC lowers 3 * n to a shift and two adds.

static volatile unsigned input = 27;

static unsigned collatz_steps(unsigned n) {
    unsigned steps = 0;
    while (n != 1) {
        n = (n & 1) ? 3 * n + 1 : n / 2;
        steps++;
    }
    return steps;
}

int main(void) {
    return (int)collatz_steps(input);
}
