import matplotlib.pyplot as plt
import numpy as np

# Range n from 1 to 100
n_values = np.arange(1, 101)

# Calculate partial sum of harmonic series: sum(1/x)
harmonic_series = np.cumsum(1.0 / n_values)

# Calculate ln(n)
ln_n = np.log(n_values)

# Plotting
plt.figure(figsize=(10, 6))
plt.plot(
    n_values,
    harmonic_series,
    label="Harmonic Sum $\sum_{i=1}^n 1/i$",
    color="blue",
    marker=".",
    markersize=6,
)
plt.plot(
    n_values,
    ln_n,
    label="Natural Log $\ln(n)$",
    color="red",
    linestyle="--",
    linewidth=2,
)

plt.xlabel("n")
plt.ylabel("Value")
plt.title("Harmonic Series vs Natural Logarithm")
plt.legend()
plt.grid(True, linestyle=":", alpha=0.6)

print("Displaying plot... (Close the window to finish)")
plt.show()
