// Unit tests for web/: plugins and src/lib helpers. Playwright owns e2e/.
// Babel here is Jest-only, so it does not touch the Docusaurus build.
/** @type {import('jest').Config} */
module.exports = {
  testEnvironment: 'node',
  testMatch: ['<rootDir>/src/**/*.test.{js,ts}', '<rootDir>/plugins/**/*.test.{js,ts}'],
  transform: {
    '^.+\\.[jt]sx?$': [
      'babel-jest',
      {
        configFile: false,
        babelrc: false,
        presets: [['@babel/preset-env', {targets: {node: 'current'}}], '@babel/preset-typescript'],
      },
    ],
  },
};
