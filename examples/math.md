# Math, right in your terminal

Inline math: $\alpha + \beta$, $x^{n+1}$, and $\nabla_\theta\mathcal{L}$.

## Probability · Gaussian normalization

$$
\int_{-\infty}^{\infty} \frac{e^{-\frac{(x-\mu)^2}{2\sigma^2}}}{\sqrt{2\pi\sigma^2}}\,dx = 1
$$

## Machine learning · Scaled dot-product attention

$$
\mathrm{Attention}(Q,K,V) = \mathrm{softmax}\left(\frac{QK^\top}{\sqrt{d_k}}\right)V
$$

## Optimization · Cross-entropy loss

$$
\mathcal{L}(\theta)=-\frac{1}{N}\sum_{i=1}^{N}\log\frac{e^{z_{i,y_i}}}{\sum_{j=1}^{K}e^{z_{i,j}}}
$$

## Linear algebra · Covariance matrix

$$
\Sigma = \begin{pmatrix} \sigma_1^2 & \rho\sigma_1\sigma_2 & 0 \\ \rho\sigma_1\sigma_2 & \sigma_2^2 & 0 \\ 0 & 0 & \sigma_3^2 \end{pmatrix}
$$

Code stays code: `price=$HOME` and `x^2`.
