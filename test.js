const cache = [1, 1];
const fib_fast = n => {
  if (cache[n] !== undefined) {
     return cache[n]
  }
  const val = fib_fast(n - 1) + fib_fast(n - 2);
  if (n > cache.length) {
    cache.length = n;
  }
  cache[n] = val;
  return val;
}

const fib_slow = n => {
  if (n < 2) {
    return 1;
  }
  return fib_slow(n - 1) + fib_slow(n - 2);
}


const fact = n => {
  if (n < 2) {
    return 1;
  }
  return n * fact(n - 1);
}

for (let i = 0; i < 100; i++) {
  console.log(fact(i));
}